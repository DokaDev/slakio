//! Slack's messages as the model's: in order, each once, the deleted ones left out, and the bots
//! that posted named by the name they posted as.

use crate::wire;
use slakio_core::model::{Message, Org, Presence, Reaction, ThreadSummary, Ts, User, UserId, WorkspaceId};
use slakio_core::sanitize::Remote;
use std::collections::HashMap;

/// A bot that posted: the name it posted as, and its team when Slack gave one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Bot {
    pub name: String,
    pub team: Option<String>,
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
                    bots.insert(author.clone(), Bot { name: name.clone(), team });
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
    }
}

fn message(m: wire::Message, me: Option<&UserId>) -> Option<Message> {
    // A deleted message's leftover: gone. (A deleted thread's own message stays as Slack's
    // `tombstone`, its replies still under it.)
    if m.hidden == Some(true) || m.subtype.as_deref() == Some("message_deleted") {
        return None;
    }
    let ts = Ts(wire::ts(&m.ts)?);
    let thread = match (m.reply_count, m.latest_reply.as_deref().and_then(wire::ts)) {
        (Some(replies), Some(last)) if replies > 0 => Some(ThreadSummary { replies, last_reply: Ts(last) }),
        _ => None,
    };
    let reactions = m
        .reactions
        .unwrap_or_default()
        .into_iter()
        .map(|r| {
            let users = r.users.unwrap_or_default();
            let mine = me.is_some_and(|me| users.iter().any(|u| u == me.as_str()));
            let count = r.count.unwrap_or_else(|| u32::try_from(users.len()).unwrap_or(u32::MAX));
            Reaction { name: Remote::new(r.name), count, mine }
        })
        .collect();
    let files: Vec<String> =
        m.files.unwrap_or_default().into_iter().filter_map(|f| f.name.or(f.title)).filter(|n| !n.is_empty()).collect();
    let text =
        crate::blocks::text(m.text.as_deref().unwrap_or_default(), m.blocks.as_ref(), m.attachments.as_ref(), &files);
    Some(Message {
        ts,
        user: UserId::new(m.user.or(m.bot_id).unwrap_or_default()),
        text: Remote::new(text),
        thread,
        reactions,
        edited: m.edited.is_some(),
    })
}
