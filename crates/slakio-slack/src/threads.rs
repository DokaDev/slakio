//! Threads read this session. Slack pages a thread forward only (`conversations.replies`, from
//! its own message on, by cursor), but the UI asks for a thread as for a conversation: its newest
//! page first, then older ones. So the newest page reads the thread from its start
//! ([`BATCH`] replies a request, [`CALLS`] requests at most) and keeps it; its older pages are
//! served from what was kept, without a request. Opening it again reads it again (new and
//! edited replies). At most [`KEPT`] threads are kept, the ones read longest ago dropped first.
//!
//! A thread longer than [`CALLS`] × [`BATCH`] is kept up to there: its "newest" page is then the
//! last one read, not the thread's last.

use crate::adapter::{Bots, read};
use crate::client::Client;
use crate::error::SlackError;
use crate::governor::Priority;
use crate::method::Method;
use crate::request::Call;
use crate::wire;
use slakio_core::model::{Message, Ts, UserId};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};

/// Replies asked for in one request.
pub const BATCH: u32 = 200;

/// Requests one reading of a thread makes at most.
pub const CALLS: usize = 15;

/// Threads kept at most.
pub const KEPT: usize = 64;

/// A thread: its conversation and its own message's time.
type Key = (String, Ts);

/// The threads kept, and the order they were read in (oldest first).
type Kept = (HashMap<Key, (Vec<Message>, Bots)>, VecDeque<Key>);

#[derive(Debug, Default)]
pub(crate) struct Threads {
    kept: Mutex<Kept>,
}

impl Threads {
    /// Up to `limit` messages of the thread at `thread` of `channel` older than `before` (the
    /// newest: `None`), oldest first; whether its start is among them; the bots that posted.
    pub(crate) async fn page(
        &self,
        client: &Client,
        me: Option<&UserId>,
        channel: &str,
        thread: Ts,
        before: Option<Ts>,
        limit: u32,
    ) -> Result<(Vec<Message>, bool, Bots), SlackError> {
        let key = (channel.to_string(), thread);
        let known = self.kept.lock().unwrap_or_else(PoisonError::into_inner).0.contains_key(&key);
        if before.is_none() || !known {
            let read = walk(client, me, channel, thread).await?;
            self.keep(key.clone(), read);
        }
        let kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        let (all, bots) = kept.0.get(&key).cloned().unwrap_or_default();
        let end = before.map_or(all.len(), |b| all.partition_point(|m| m.ts < b));
        let start = end.saturating_sub(limit as usize);
        Ok((all[start..end].to_vec(), start == 0, bots))
    }

    fn keep(&self, key: Key, read: (Vec<Message>, Bots)) {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        let (map, order) = &mut *kept;
        order.retain(|k| *k != key);
        order.push_back(key.clone());
        map.insert(key, read);
        while order.len() > KEPT {
            if let Some(old) = order.pop_front() {
                map.remove(&old);
            }
        }
    }
}

/// The thread at `thread` of `channel`, read from its start: its messages oldest first (its own
/// message first), and the bots that posted.
async fn walk(
    client: &Client,
    me: Option<&UserId>,
    channel: &str,
    thread: Ts,
) -> Result<(Vec<Message>, Bots), SlackError> {
    let (ts, limit) = (thread.to_string(), BATCH.to_string());
    let mut raw: Vec<wire::Message> = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..CALLS {
        let mut params = vec![("channel", channel), ("ts", ts.as_str()), ("limit", limit.as_str())];
        if let Some(c) = &cursor {
            params.push(("cursor", c.as_str()));
        }
        let page: wire::History =
            client.call(&Call::form(Method::ConversationsReplies, &params), Priority::User).await?;
        let next = page.next_cursor().map(str::to_string);
        let more = page.has_more == Some(true);
        let got = page.messages.unwrap_or_default();
        let empty = got.is_empty();
        raw.extend(got);
        // The end: Slack says so, gives no cursor, the same cursor again, or nothing at all.
        match next {
            Some(n) if more && !empty && cursor.as_deref() != Some(n.as_str()) => cursor = Some(n),
            _ => break,
        }
    }
    Ok(read(raw, me))
}
