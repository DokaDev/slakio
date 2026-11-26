//! Slack's messages as the model's: in order, each once, the deleted ones left out, and the bots
//! that posted named by the name they posted as.

use crate::wire;
use slakio_core::model::{
    EMOJI_PICTURES_MAX, File, FileId, Message, Org, Presence, Reaction, System, ThreadSummary, Ts, User, UserId,
    WorkspaceId,
};
use slakio_core::sanitize::Remote;
use std::collections::HashMap;

/// A bot that posted: the name it posted as, its team and its picture when Slack gave them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Bot {
    pub name: String,
    pub team: Option<String>,
    pub icon: Option<String>,
}

/// The bots that posted in a page, by the author id their messages carry: one for each bot and
/// name it posted as, so a bot that posts under another name later does not rename its earlier
/// messages.
pub(crate) type Bots = HashMap<String, Bot>;

/// The author id of the messages bot `id` posted as `name`: the bot's id and a hash of the name
/// (no character of the name in it).
fn bot_author(id: &str, name: &str) -> String {
    // FNV-1a: the same id every run (the cache keeps it).
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("{id}~{hash:016x}")
}

/// `raw` as messages, oldest first, each once (of a message Slack lists twice, the copy edited
/// last), the deleted ones left out; and the bots among their authors.
pub(crate) fn read(raw: Vec<wire::Message>, me: Option<&UserId>) -> (Vec<Message>, Bots) {
    let mut bots = Bots::new();
    let mut messages: Vec<(Message, u64)> =
        raw.into_iter()
            .filter_map(|mut m| {
                if let (None, Some(id), Some(name)) = (&m.user, &m.bot_id, &m.username) {
                    let author = bot_author(id, name);
                    let team = m.bot_profile.as_ref().and_then(|b| b.team_id.clone()).or_else(|| m.team.clone());
                    // The message's own picture first, else the bot's; one known stays.
                    let icons = [m.icons.as_ref(), m.bot_profile.as_ref().and_then(|b| b.icons.as_ref())];
                    let icon = icons.into_iter().flatten().find_map(wire::Icons::photo);
                    let bot = bots.entry(author.clone()).or_default();
                    (bot.name, bot.team) = (name.clone(), team);
                    bot.icon = icon.or(bot.icon.take());
                    m.user = Some(author);
                }
                // When it was edited last: 0 never, 1 at a time Slack did not give.
                let edited = m.edited.as_ref().map_or(0, |e| {
                    e.get("ts").and_then(serde_json::Value::as_str).and_then(wire::ts).unwrap_or(1).max(1)
                });
                message(m, me).map(|m| (m, edited))
            })
            .collect();
    messages.sort_by_key(|(m, _)| m.ts);
    messages.dedup_by(|later, kept| {
        if later.0.ts != kept.0.ts {
            return false;
        }
        if later.1 > kept.1 {
            std::mem::swap(later, kept);
        }
        true
    });
    (messages.into_iter().map(|(m, _)| m).collect(), bots)
}

/// A bot of workspace `ws` as a person: named as it posts; of another organization when its
/// team is another than `ws`.
pub(crate) fn bot(ws: &str, id: String, b: Bot) -> User {
    let org = match b.team {
        Some(t) if !t.is_empty() && t != ws => Org::External(Remote::new(t)),
        _ => Org::Own,
    };
    User {
        id: UserId::new(id),
        workspace: WorkspaceId::new(ws),
        name: Remote::new(b.name.clone()),
        display_name: Remote::new(b.name),
        org,
        bot: true,
        presence: Presence::Unknown,
        avatar: b.icon,
        no_photo: false,
        avatar_guessed: false,
        status: None,
        no_status: false,
    }
}

fn message(m: wire::Message, me: Option<&UserId>) -> Option<Message> {
    // A deleted message's leftover: gone. (A deleted thread's own message stays as Slack's
    // `tombstone`, its replies still under it.)
    if m.hidden == Some(true) || m.subtype.as_deref() == Some("message_deleted") {
        return None;
    }
    let ts = Ts(wire::ts(&m.ts)?);
    let system = system(&m).map(Box::new);
    let thread = match (m.reply_count, m.latest_reply.as_deref().and_then(wire::ts)) {
        (Some(replies), Some(last)) if replies > 0 => Some(ThreadSummary { replies, last_reply: Ts(last) }),
        _ => None,
    };
    let reactions = m.reactions.unwrap_or_default();
    let given = reactions.iter().filter_map(|r| Some((r.name.clone(), r.url.clone()?)));
    let blocks = crate::blocks::emoji_pictures(m.blocks.as_ref(), EMOJI_PICTURES_MAX);
    let emoji_pictures = emoji_pictures(given.chain(blocks));
    let reactions = reactions
        .into_iter()
        .map(|r| {
            let users = r.users.unwrap_or_default();
            let mine = me.is_some_and(|me| users.iter().any(|u| u == me.as_str()));
            let count = r.count.unwrap_or_else(|| u32::try_from(users.len()).unwrap_or(u32::MAX));
            Reaction { name: Remote::new(r.name), count, mine }
        })
        .collect();
    let files: Vec<File> = m.files.unwrap_or_default().into_iter().filter_map(file).collect();
    let text = crate::blocks::text(m.text.as_deref().unwrap_or_default(), m.blocks.as_ref(), m.attachments.as_ref());
    Some(Message {
        ts,
        user: UserId::new(m.user.or(m.bot_id).unwrap_or_default()),
        text: Remote::new(text),
        thread,
        reactions,
        edited: m.edited.is_some(),
        files,
        emoji_pictures,
        system,
    })
}

/// What a notice of Slack's about the conversation tells, by its subtype (a public channel's
/// `channel_*` or a private one's `group_*`); `None` for anything else, which shows as written.
fn system(m: &wire::Message) -> Option<System> {
    let text = |t: &Option<String>| Remote::new(t.clone().unwrap_or_default());
    let subtype = m.subtype.as_deref()?;
    let change = subtype.strip_prefix("channel_").or_else(|| subtype.strip_prefix("group_"));
    Some(match (subtype, change) {
        (_, Some("join")) => System::Joined,
        (_, Some("leave")) => System::Left,
        (_, Some("topic")) => System::Topic(text(&m.topic)),
        (_, Some("purpose")) => System::Purpose(text(&m.purpose)),
        (_, Some("name")) => System::Renamed { old: text(&m.old_name), new: text(&m.name) },
        (_, Some("archive")) => System::Archived,
        (_, Some("unarchive")) => System::Unarchived,
        ("pinned_item", _) => System::Pinned,
        ("unpinned_item", _) => System::Unpinned,
        ("bot_add", _) => System::AppAdded,
        ("bot_remove", _) => System::AppRemoved,
        ("channel_convert_to_private", _) => System::MadePrivate,
        ("channel_convert_to_public", _) => System::MadePublic,
        _ => return None,
    })
}

/// The emoji addresses a message gives, each kept only for a name a shortcode may have (its
/// Hangul composed) at an address on the emoji host, once a name, at most
/// [`EMOJI_PICTURES_MAX`].
fn emoji_pictures(given: impl Iterator<Item = (String, String)>) -> Vec<(Remote, String)> {
    use slakio_core::images::emoji::{compose_hangul, shortcode};
    let mut out: Vec<(Remote, String)> = Vec::new();
    for (name, url) in given {
        let name = compose_hangul(&name).into_owned();
        let Some(address) = slakio_core::images::emoji_address(&url).filter(|_| shortcode(&name)) else { continue };
        if out.len() < EMOJI_PICTURES_MAX && !out.iter().any(|(n, _)| n.unsanitized() == name) {
            out.push((Remote::new(name), address));
        }
    }
    out
}

/// A file Slack lists with a message, when it has an id: named (its title when it has no
/// name), its kind in words, its size, and where it is downloaded from when it can be (a file
/// hosted on Slack's file host, not deleted, hidden or hosted elsewhere); for a picture that
/// can be, the smaller copy it is previewed by ([`thumb`]).
fn file(f: wire::File) -> Option<File> {
    let id = f.id.clone().filter(|id| !id.is_empty())?;
    let thumb = thumb(&f);
    let name = f.name.filter(|n| !n.is_empty()).or(f.title).unwrap_or_default();
    let kind = f.pretty_type.filter(|k| !k.is_empty()).or(f.filetype).unwrap_or_default();
    let gone = matches!(f.mode.as_deref(), Some("tombstone" | "hidden_by_limit" | "external"));
    let download = f
        .url_private_download
        .filter(|_| !gone && f.is_external != Some(true))
        .and_then(|u| crate::download::address(&u));
    let size = f.size.as_ref().and_then(serde_json::Value::as_u64);
    let thumb = thumb.filter(|_| download.is_some());
    Some(File { id: FileId::new(id), name: Remote::new(name), kind: Remote::new(kind), size, download, thumb })
}

/// The smaller copy a picture (PNG, JPEG, GIF) is previewed by, on the file host: the 480-pixel
/// one, else the 360, the 720 or the 160; never the original.
fn thumb(f: &wire::File) -> Option<String> {
    if !f.filetype.as_deref().is_some_and(slakio_core::images::previewed) {
        return None;
    }
    [&f.thumb_480, &f.thumb_360, &f.thumb_720, &f.thumb_160]
        .into_iter()
        .find_map(|t| t.as_deref().and_then(slakio_core::images::thumb_address))
}

/// What a message lists in words where only its text is shown (Activity): its text, or the
/// names of its files when it has no text.
pub(crate) fn summary(m: &Message) -> Remote {
    if !m.text.unsanitized().trim().is_empty() || m.files.is_empty() {
        return m.text.clone();
    }
    let names: Vec<&str> = m.files.iter().map(|f| f.name.unsanitized()).collect();
    Remote::new(format!("\u{1F4CE} {}", names.join(", ")))
}
