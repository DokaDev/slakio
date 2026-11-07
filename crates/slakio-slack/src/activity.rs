//! The user's Activity (`activity.feed`), a page at a time and only when the UI asks: what each
//! notice is about comes by id (a conversation, a message, a thread), and the texts of those
//! messages are looked up by id with `messages.list`, at most [`CHANNELS_PER_CALL`] conversations
//! a request and [`TEXT_CALLS_MAX`] requests a page. Nothing is marked read: Activity is read
//! as it is. A text that cannot be read is unknown (the notice is shown without it), never
//! empty; only a refused sign-in fails the page.
//!
//! Derived from gammons/slk (MIT, Copyright (c) 2026 Grant Ammons): the parameters of
//! `activity.feed` as the web client sends them, the shapes of its notices by type, and the
//! texts looked up with `messages.list`, a few conversations a request.

use crate::adapter::Session;
use crate::client::Client;
use crate::error::SlackError;
use crate::governor::Priority;
use crate::messages::{Bots, bot, read};
use crate::method::Method;
use crate::request::Call;
use crate::search::shaped;
use crate::wire;
use slakio_core::backend::{ACTIVITY_PAGE, CURSOR_MAX, Feed, Notice, NoticeKind};
use slakio_core::model::{ConversationId, Message, Ts, UserId, WorkspaceId};
use slakio_core::sanitize::Remote;
use std::collections::{BTreeMap, HashMap};
use std::sync::PoisonError;

// derived from gammons/slk (MIT)
/// The kinds of notices asked for: the web client's own list (the ones Activity does not show
/// are left out when read).
pub const TYPES: &str = "at_user,at_user_group,at_channel,at_everyone,keyword,thread_v2,message_reaction,bot_dm_bundle,dm,unjoined_channel_mention,channel,saved_reminder,list_user_mentioned";

/// The most conversations one `messages.list` names (Slack refuses many more).
pub const CHANNELS_PER_CALL: usize = 8;

/// The most `messages.list` requests for one page of Activity.
pub const TEXT_CALLS_MAX: usize = 3;

/// The most characters of a notice's key kept: a longer one is no key.
const KEY_MAX: usize = 256;

/// A cursor of Slack's shape: printable ASCII, not too long, not empty.
pub(crate) fn cursor_ok(c: &str) -> bool {
    !c.is_empty() && c.len() <= CURSOR_MAX && c.bytes().all(|b| b.is_ascii_graphic())
}

// derived from gammons/slk (MIT)
/// The page of the user's Activity after `cursor` (the newest without one), in the user's share
/// of the request budget, with the texts of its messages and their authors by name. A cursor
/// not of Slack's shape asks nothing: the page is empty and the last.
pub(crate) async fn activity(client: &Client, ctx: &Session, cursor: Option<&str>) -> Result<Feed, SlackError> {
    let workspace = WorkspaceId::new(ctx.people.team().unwrap_or_default());
    let after = cursor.map(str::to_string);
    if cursor.is_some_and(|c| !cursor_ok(c)) {
        return Ok(Feed { workspace, after, notices: Vec::new(), next: None, users: Vec::new() });
    }
    let limit = ACTIVITY_PAGE.to_string();
    let mut params = vec![
        ("limit", limit.as_str()),
        ("types", TYPES),
        ("mode", "chrono_v1"),
        ("archive_only", "false"),
        ("unread_only", "false"),
        ("priority_only", "false"),
        ("exclude_automations", "false"),
    ];
    if let Some(c) = cursor {
        params.push(("cursor", c));
    }
    let answer: wire::Activity = client.call(&Call::form(Method::ActivityFeed, &params), Priority::User).await?;
    let next = (answer.response_metadata.and_then(|m| m.next_cursor)).filter(|c| cursor_ok(c));
    let mut notices: Vec<Notice> = Vec::new();
    for n in answer.items.unwrap_or_default().into_iter().filter_map(notice) {
        if notices.len() < ACTIVITY_PAGE as usize && !notices.iter().any(|k| k.key == n.key) {
            notices.push(n);
        }
    }
    let me = ctx.me.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let (texts, bots) = texts(client, &notices, me.as_ref()).await?;
    for n in &mut notices {
        if let Some(m) = texts.get(&(n.conversation.clone(), n.ts)) {
            n.text = Some(m.text.clone());
            // A reaction names who reacted; the others the message's author.
            if n.kind != NoticeKind::Reaction && (n.user.as_str().is_empty() || bots.contains_key(m.user.as_str())) {
                n.user = m.user.clone();
            }
        }
    }
    let authors: Vec<String> = (notices.iter().map(|n| n.user.as_str().to_string()))
        .filter(|u| !u.is_empty() && me.as_ref().is_none_or(|me| me.as_str() != u) && !bots.contains_key(u))
        .collect();
    let mut users = ctx.people.names(client, &authors, Priority::User).await;
    if let Some(ws) = ctx.people.team() {
        users.extend(bots.into_iter().map(|(id, b)| bot(&ws, id, b)));
    }
    Ok(Feed { workspace, after, notices, next, users })
}

/// Where a message is: its conversation, its time.
type Place = (ConversationId, Ts);

/// The texts of the messages `notices` are about, read with `messages.list`; the bots that
/// wrote them. Fails only for a refused sign-in: anything else leaves those texts unknown.
async fn texts(
    client: &Client,
    notices: &[Notice],
    me: Option<&UserId>,
) -> Result<(HashMap<Place, Message>, Bots), SlackError> {
    let mut wanted: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for n in notices {
        let ts = wanted.entry(n.conversation.as_str()).or_default();
        let t = n.ts.to_string();
        if !ts.contains(&t) {
            ts.push(t);
        }
    }
    let groups: Vec<serde_json::Value> =
        wanted.into_iter().map(|(c, ts)| serde_json::json!({"channel": c, "timestamps": ts})).collect();
    let (mut out, mut bots) = (HashMap::new(), Bots::new());
    for batch in groups.chunks(CHANNELS_PER_CALL).take(TEXT_CALLS_MAX) {
        let ids = serde_json::Value::Array(batch.to_vec()).to_string();
        let params = [("message_ids", ids.as_str()), ("org_wide_aware", "true"), ("cached_latest_updates", "{}")];
        let answer =
            match client.call::<wire::MessagesList>(&Call::form(Method::MessagesList, &params), Priority::User).await {
                Ok(a) => a,
                Err(e @ SlackError::Auth { .. }) => return Err(e),
                Err(_) => continue,
            };
        for (channel, history) in answer.messages_data.unwrap_or_default() {
            let Some(history) =
                shaped(&channel, b"CGD").then(|| serde_json::from_value::<wire::History>(history).ok()).flatten()
            else {
                continue;
            };
            let (messages, more) = read(history.messages.unwrap_or_default(), me);
            bots.extend(more);
            for m in messages {
                out.insert((ConversationId::new(&channel), m.ts), m);
            }
        }
    }
    Ok((out, bots))
}

/// A Slack id of `first`'s shape, else none.
fn id(v: Option<String>, first: &[u8]) -> Option<String> {
    v.filter(|v| shaped(v, first))
}

/// A notice as Activity shows it; `None` for a kind it does not show, or one without a
/// conversation and a message of Slack's shape.
fn notice(raw: wire::ActivityItem) -> Option<Notice> {
    let key = raw.key;
    if key.is_empty() || key.len() > KEY_MAX {
        return None;
    }
    let inner = raw.item?;
    let kind = match inner.kind.as_deref()? {
        "at_user" | "unjoined_channel_mention" => NoticeKind::Mention,
        "at_channel" | "at_everyone" => NoticeKind::Everyone,
        "at_user_group" => NoticeKind::Group,
        "keyword" => NoticeKind::Keyword,
        "thread_v2" => NoticeKind::Thread,
        "message_reaction" => NoticeKind::Reaction,
        "dm" | "bot_dm_bundle" => NoticeKind::Dm,
        _ => return None,
    };
    let payload = inner.bundle_info.and_then(|b| b.payload).unwrap_or_default();
    // (conversation, message, thread, author)
    let (conversation, ts, thread, mut user) = match kind {
        NoticeKind::Thread => {
            let t = payload.thread_entry?;
            (t.channel_id, t.latest_ts, t.thread_ts, None)
        }
        NoticeKind::Dm => {
            let m = payload.dm_entry.and_then(|d| d.latest_message).or(payload.message)?;
            (m.channel, m.ts, None, m.author_user_id)
        }
        _ => {
            let m = inner.message?;
            (m.channel, m.ts, m.thread_ts, m.author_user_id)
        }
    };
    let mut reaction = None;
    if kind == NoticeKind::Reaction {
        let r = inner.reaction?;
        user = r.user;
        reaction = Some(Remote::new(r.name?.chars().take(100).collect::<String>()));
    }
    let conversation = ConversationId::new(id(conversation, b"CGD")?);
    let ts = Ts(wire::ts(&ts?)?);
    // The message itself is not a reply under itself.
    let thread = thread.and_then(|t| wire::ts(&t)).map(Ts).filter(|t| *t != ts);
    let at = raw.feed_ts.as_deref().and_then(wire::ts).map_or(ts, Ts);
    Some(Notice {
        key,
        kind,
        at,
        conversation,
        ts,
        thread,
        user: UserId::new(id(user, b"UWB").unwrap_or_default()),
        text: None,
        reaction,
        unread: raw.is_unread == Some(true),
    })
}

#[cfg(test)]
mod tests;
