//! Threads read this session. Slack pages a thread forward only (`conversations.replies`, from
//! its own message on, by cursor), but the UI asks for a thread as for a conversation: its newest
//! page first, then older ones. So the first newest page reads the thread from its start
//! ([`BATCH`] replies a request, [`CALLS`] requests at most) and keeps it; its older pages are
//! served from what was kept, without a request.
//!
//! Opening a kept thread again reads only what is new: the replies from the last one kept on
//! (one request, mostly), or the whole thread again when it fits in one request (its edits too).
//! A thread read only in part is not read again while it is kept. So opening threads again and
//! again cannot spend the hour's requests. At most [`KEPT`] threads and [`KEPT_BYTES`] of their
//! text are kept, the ones read longest ago dropped first. A reading that fails keeps nothing of
//! it: what was kept before stays as it was.
//!
//! A thread longer than [`CALLS`] × [`BATCH`] replies, or one Slack answers with more to come but
//! no way on, is read only in part ("cut"): its "newest" page is the last one read, not the
//! thread's last, and its pages say so.

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

/// The most bytes of text the threads kept hold (each message counted with [`OVERHEAD`] more).
pub const KEPT_BYTES: usize = 64 << 20;

/// What a kept message costs besides its text.
const OVERHEAD: usize = 128;

/// A thread: its conversation and its own message's time.
type Key = (String, Ts);

/// A thread as read: its messages oldest first, the bots that posted, whether it was cut.
#[derive(Clone, Debug, Default)]
pub(crate) struct Walked {
    pub messages: Vec<Message>,
    pub bots: Bots,
    pub cut: bool,
}

impl Walked {
    fn bytes(&self) -> usize {
        self.messages.iter().map(|m| m.text.unsanitized().len() + OVERHEAD).sum()
    }
}

/// The threads kept, the order they were used in (oldest first), the bytes they hold, and the
/// most of each.
#[derive(Debug)]
struct Kept {
    map: HashMap<Key, (Walked, usize)>,
    order: VecDeque<Key>,
    bytes: usize,
    most: (usize, usize),
}

#[derive(Debug)]
pub(crate) struct Threads {
    kept: Mutex<Kept>,
}

impl Default for Threads {
    fn default() -> Self {
        Self::with_limits(KEPT, KEPT_BYTES)
    }
}

/// A page of a thread: its messages (oldest first), whether its start is among them, the bots
/// that posted, whether the thread was cut.
pub(crate) type ThreadPage = (Vec<Message>, bool, Bots, bool);

/// What a page needs read first.
enum Read {
    /// Nothing: it is kept.
    Kept,
    /// The thread from its start.
    Whole,
    /// The replies from the newest kept on, onto what was kept.
    From(Ts, Walked),
}

impl Threads {
    /// At most `threads` threads and `bytes` bytes kept.
    pub(crate) fn with_limits(threads: usize, bytes: usize) -> Self {
        let kept = Kept { map: HashMap::new(), order: VecDeque::new(), bytes: 0, most: (threads, bytes) };
        Self { kept: Mutex::new(kept) }
    }

    /// Up to `limit` messages of the thread at `thread` of `channel` older than `before` (the
    /// newest: `None`).
    pub(crate) async fn page(
        &self,
        client: &Client,
        me: Option<&UserId>,
        channel: &str,
        thread: Ts,
        before: Option<Ts>,
        limit: u32,
    ) -> Result<ThreadPage, SlackError> {
        let key = (channel.to_string(), thread);
        let read_first = {
            let kept = self.lock();
            match kept.map.get(&key) {
                None => Read::Whole,
                Some((t, _)) if before.is_some() || t.cut => Read::Kept,
                Some((t, _)) if t.messages.len() <= BATCH as usize => Read::Whole,
                Some((t, _)) => t.messages.last().map_or(Read::Whole, |m| Read::From(m.ts, t.clone())),
            }
        };
        match read_first {
            Read::Kept => {}
            Read::Whole => {
                let walked = walk(client, me, channel, thread, None).await?;
                self.keep(key.clone(), walked);
            }
            Read::From(from, old) => {
                let new = walk(client, me, channel, thread, Some(from)).await?;
                self.keep(key.clone(), merged(old, new, from));
            }
        }
        Ok(self.slice(&key, before, limit))
    }

    /// Who wrote in the kept thread at `thread` of `channel`, each once (none when it is not
    /// kept).
    pub(crate) fn authors(&self, channel: &str, thread: Ts) -> Vec<String> {
        let kept = self.lock();
        let Some((t, _)) = kept.map.get(&(channel.to_string(), thread)) else { return Vec::new() };
        let mut out: Vec<String> = t.messages.iter().map(|m| m.user.as_str().to_string()).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The page of the kept thread `key` ending before `before` (the end: `None`), used now.
    fn slice(&self, key: &Key, before: Option<Ts>, limit: u32) -> ThreadPage {
        let mut kept = self.lock();
        if kept.map.contains_key(key) {
            kept.order.retain(|k| k != key);
            kept.order.push_back(key.clone());
        }
        let Some((t, _)) = kept.map.get(key) else { return (Vec::new(), true, Bots::new(), false) };
        let end = before.map_or(t.messages.len(), |b| t.messages.partition_point(|m| m.ts < b));
        let start = end.saturating_sub(limit as usize);
        (t.messages[start..end].to_vec(), start == 0, t.bots.clone(), t.cut)
    }

    /// Keep `walked` as thread `key`, dropping the threads used longest ago while there are too
    /// many or they hold too much (never the one just kept).
    fn keep(&self, key: Key, walked: Walked) {
        let mut kept = self.lock();
        let kept = &mut *kept;
        let bytes = walked.bytes();
        if let Some((_, old)) = kept.map.insert(key.clone(), (walked, bytes)) {
            kept.bytes -= old;
        }
        kept.bytes += bytes;
        kept.order.retain(|k| *k != key);
        kept.order.push_back(key);
        while kept.order.len() > 1 && (kept.order.len() > kept.most.0 || kept.bytes > kept.most.1) {
            if let Some(old) = kept.order.pop_front()
                && let Some((_, b)) = kept.map.remove(&old)
            {
                kept.bytes -= b;
            }
        }
    }

    #[cfg(test)]
    fn kept(&self) -> (Vec<Key>, usize) {
        let kept = self.lock();
        (kept.order.iter().cloned().collect(), kept.bytes)
    }
}

/// What was kept, `old`, with what was read from `from` on, `new` (its copies win).
fn merged(old: Walked, new: Walked, from: Ts) -> Walked {
    let mut messages = new.messages;
    messages.extend(old.messages.into_iter().filter(|m| m.ts < from));
    messages.sort_by_key(|m| m.ts);
    messages.dedup_by_key(|m| m.ts);
    let mut bots = old.bots;
    bots.extend(new.bots);
    Walked { messages, bots, cut: new.cut }
}

/// The thread at `thread` of `channel`, read from its start (or from `from` on): its messages
/// oldest first (its own message first), the bots that posted, and whether it ended before the
/// thread did: [`CALLS`] requests, or more to come with no way on.
async fn walk(
    client: &Client,
    me: Option<&UserId>,
    channel: &str,
    thread: Ts,
    from: Option<Ts>,
) -> Result<Walked, SlackError> {
    let (ts, limit, oldest) = (thread.to_string(), BATCH.to_string(), from.map(|t| t.to_string()));
    let mut raw: Vec<wire::Message> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut cut = true;
    for _ in 0..CALLS {
        let mut params = vec![("channel", channel), ("ts", ts.as_str()), ("limit", limit.as_str())];
        if let Some(o) = &oldest {
            params.extend([("oldest", o.as_str()), ("inclusive", "true")]);
        }
        if let Some(c) = &cursor {
            params.push(("cursor", c.as_str()));
        }
        let page: wire::History =
            client.call(&Call::form(Method::ConversationsReplies, &params), Priority::User).await?;
        let next = page.next_cursor().map(str::to_string);
        let more = page.has_more;
        let got = page.messages.unwrap_or_default();
        let empty = got.is_empty();
        raw.extend(got);
        // The end: Slack says so. Anything else that stops the reading (no cursor, the same
        // cursor again, nothing at all, no word on more) leaves the rest unread.
        match next {
            Some(n) if more == Some(true) && !empty && cursor.as_deref() != Some(n.as_str()) => cursor = Some(n),
            _ => {
                cut = more != Some(false);
                break;
            }
        }
    }
    let (messages, bots) = read(raw, me);
    Ok(Walked { messages, bots, cut })
}

#[cfg(test)]
mod tests;
