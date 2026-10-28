//! What the live updates' socket says, read tolerantly: the frames the client acts on — `hello`,
//! `goodbye`, a message posted, changed or deleted, a conversation read elsewhere — and nothing
//! else (presence, typing, reactions and the rest are left unread for now). A frame that cannot
//! be read, or that lacks what it needs (a channel, a timestamp, the counts), is left out:
//! unknown is not zero.
//!
//! Derived from gammons/slk (MIT, Copyright (c) 2026 Grant Ammons): the event names, which
//! message subtypes are a new message, an edit or a deletion, and the `*_marked` fields.

use crate::adapter::{Bots, read};
use crate::wire;
use serde_json::Value;
use slakio_core::model::{Message, Ts, UserId};

/// One frame of the socket, as the client acts on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Frame {
    /// The socket is ready: live updates arrive from now on.
    Hello,
    /// Slack ends the socket soon (and the client connects again).
    Goodbye,
    /// A message posted (or, `changed`, a new copy of one) in `conversation`.
    Message {
        conversation: String,
        thread: Option<Ts>,
        broadcast: bool,
        changed: bool,
        mention: bool,
        message: Message,
        bots: Bots,
    },
    Deleted {
        conversation: String,
        thread: Option<Ts>,
        ts: Ts,
    },
    Marked {
        conversation: String,
        unread: u32,
        mentions: u32,
    },
    /// Anything else.
    Other,
}

/// The frame `text` says, for the user `me`.
pub(crate) fn frame(text: &str, me: Option<&UserId>) -> Frame {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Frame::Other };
    let s = |key: &str| v.get(key).and_then(Value::as_str);
    match s("type") {
        Some("hello") => Frame::Hello,
        Some("goodbye") => Frame::Goodbye,
        Some("message") => message(&v, me).unwrap_or(Frame::Other),
        Some("channel_marked" | "im_marked" | "group_marked" | "mpim_marked") => marked(&v).unwrap_or(Frame::Other),
        _ => Frame::Other,
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn ts_of(v: &Value, key: &str) -> Option<Ts> {
    str_of(v, key).and_then(wire::ts).map(Ts)
}

fn message(v: &Value, me: Option<&UserId>) -> Option<Frame> {
    let conversation = str_of(v, "channel").filter(|c| !c.is_empty())?.to_string();
    // A message only one person sees for a moment (an app's answer to them): not shown.
    if v.get("is_ephemeral").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let subtype = str_of(v, "subtype");
    if subtype == Some("message_deleted") {
        let ts = ts_of(v, "deleted_ts")?;
        let thread = v.get("previous_message").and_then(|p| ts_of(p, "thread_ts")).filter(|t| *t != ts);
        return Some(Frame::Deleted { conversation, thread, ts });
    }
    let changed = matches!(subtype, Some("message_changed" | "message_replied"));
    let inner = if changed { v.get("message")? } else { v };
    if inner.get("is_ephemeral").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let raw: wire::Message = serde_json::from_value(inner.clone()).ok()?;
    let mention = mentions(raw.text.as_deref().unwrap_or_default(), me);
    let (mut messages, bots) = read(vec![raw], me);
    let message = messages.pop()?;
    let thread = ts_of(inner, "thread_ts").filter(|t| *t != message.ts);
    let broadcast = inner.get("subtype").and_then(Value::as_str) == Some("thread_broadcast");
    Some(Frame::Message { conversation, thread, broadcast, changed, mention, message, bots })
}

/// `text` (Slack's markup) mentions `me`, or everyone there (`@here`, `@channel`, `@everyone`).
fn mentions(text: &str, me: Option<&UserId>) -> bool {
    let personal = me.is_some_and(|me| {
        let at = format!("<@{}", me.as_str());
        text.match_indices(&at).any(|(i, _)| matches!(text.as_bytes().get(i + at.len()), Some(b'>' | b'|')))
    });
    personal || ["<!here", "<!channel", "<!everyone"].iter().any(|w| text.contains(w))
}

fn marked(v: &Value) -> Option<Frame> {
    let conversation = str_of(v, "channel").filter(|c| !c.is_empty())?.to_string();
    let count = |key: &str| v.get(key).and_then(Value::as_u64).map(|n| u32::try_from(n).unwrap_or(u32::MAX));
    let unread = count("unread_count_display")?;
    let mentions = count("mention_count")?;
    Some(Frame::Marked { conversation, unread: unread.max(mentions), mentions })
}

#[cfg(test)]
mod tests;
