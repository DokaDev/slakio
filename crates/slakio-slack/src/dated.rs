//! A conversation read from a date (`:jump <date>`): the messages around the first one written
//! on or after it, in the user's share of the request budget, a few requests at most.
//!
//! Slack lists a conversation newest first and pages back from a bound (`latest`); asked for
//! what came after a time (`oldest` alone) it gives the newest after it, not the first. So: the
//! newest after the date are read first; when they are all there is, they join those before the
//! date (the page reaches the newest). Else the first one after the date is older than all of
//! them: a few pages ([`PROBES`]) are read that end where it likely is, at the pace the
//! messages read were written, until one holds a message after the date and one before it (so
//! the first after it among them); else the page ending at the oldest found after the date is
//! shown (reading back from it reaches the date). Every page so
//! read is whole from its end back, so no gap is ever hidden in it; its newer messages are not
//! read ([`Page::cut`]).

use crate::adapter::{PAGE_MAX, Session, history, named};
use crate::client::Client;
use crate::error::SlackError;
use crate::governor::Priority;
use crate::messages::read;
use crate::method::Method;
use crate::request::Call;
use crate::wire;
use slakio_core::backend::Page;
use slakio_core::model::{Target, Ts};
use std::sync::PoisonError;

/// How many pages are tried to find the first message after the date, at most (one request
/// each).
pub const PROBES: usize = 5;

/// The messages of `target` around the first one at `at` or after (see the module's text). A
/// thread is read as when it opens.
pub(crate) async fn dated(
    client: &Client,
    ctx: &Session,
    target: Target,
    at: Ts,
    limit: u32,
) -> Result<Page, SlackError> {
    let Target::Conversation { conversation, .. } = &target else {
        return history(client, ctx, target, None, limit).await;
    };
    let me = ctx.me.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let (channel, count) = (conversation.as_str().to_string(), limit.clamp(1, PAGE_MAX));
    let after = window(client, &channel, count, Bound::After(at)).await?;
    // The oldest of those, when there are more after the date than these.
    let more = times(&after).into_iter().min().filter(|_| after.has_more != Some(false));
    let (raw, complete, cut) = match more {
        // All there is after the date (or nothing): with those before it, up to the newest.
        None => {
            let before = window(client, &channel, count, Bound::Before(at)).await?;
            let reaches = after.has_more == Some(false);
            let mut raw = before.messages.unwrap_or_default();
            if reaches {
                raw.extend(after.messages.unwrap_or_default());
            }
            (raw, before.has_more == Some(false), !reaches)
        }
        Some(_) => {
            let page = first_page_after(client, &channel, count, at.0, &after).await?;
            (page.messages.unwrap_or_default(), page.has_more == Some(false), true)
        }
    };
    let (messages, bots) = read(raw, me.as_ref());
    let page = Page { target, messages, complete, users: Vec::new(), cut };
    Ok(named(client, ctx, page, None, bots, me.as_ref(), Priority::User).await)
}

/// The page ending just after the first message at `at` or after, `after` the newest after it
/// (a message after `at` is at `hi`, the oldest of them). A try reads the page ending about half
/// a page past the last time known to have nothing after `at` (at first `at` itself), at the
/// pace the messages last read were written; a page all after `at` moves `hi` back to its
/// oldest; one with nothing after `at` moves past it, and the next try halves the time left
/// instead (the pace said too little). Else the page ending at `hi`.
async fn first_page_after(
    client: &Client,
    channel: &str,
    count: u32,
    at: u64,
    after: &wire::History,
) -> Result<wire::History, SlackError> {
    let read = times(after);
    let (mut lo, mut hi) = (at, read.iter().copied().min().unwrap_or(at));
    let mut gap = pace(&read).unwrap_or_else(|| (hi - lo) / u64::from(count)).max(1);
    let (half, mut halve) = (u64::from(count / 2).max(1), false);
    for _ in 0..PROBES {
        if hi <= lo + 1 {
            break;
        }
        let end = if halve { lo + (hi - lo) / 2 } else { lo.saturating_add(gap.saturating_mul(half)) };
        let end = end.clamp(lo + 1, hi);
        let page = window(client, channel, count, Bound::UpTo(Ts(end))).await?;
        let read = times(&page);
        match read.iter().copied().filter(|t| *t >= at).min() {
            // Nothing from `at` up to there: the first is later.
            None => (lo, halve) = (end, true),
            Some(_) if read.iter().any(|t| *t < at) || page.has_more == Some(false) => return Ok(page),
            // All after `at`: the first is older than all of them, written at their pace.
            Some(oldest) => (hi, gap, halve) = (oldest, pace(&read).unwrap_or(gap).max(1), false),
        }
    }
    window(client, channel, count, Bound::UpTo(Ts(hi))).await
}

/// The time between two messages of `read` on average; `None` for fewer than two.
fn pace(read: &[u64]) -> Option<u64> {
    let (min, max) = (read.iter().min()?, read.iter().max()?);
    let n = u64::try_from(read.len()).ok()?.checked_sub(1).filter(|n| *n > 0)?;
    Some((max - min) / n)
}

/// Where a page of `conversations.history` is bounded.
#[derive(Clone, Copy)]
enum Bound {
    /// The newest from this time on, it included.
    After(Ts),
    /// The newest before this time.
    Before(Ts),
    /// The newest up to this time, it included.
    UpTo(Ts),
}

/// Up to `count` messages of `channel` within `bound`, as Slack lists them.
async fn window(client: &Client, channel: &str, count: u32, bound: Bound) -> Result<wire::History, SlackError> {
    let count = count.to_string();
    let (key, at, inclusive) = match bound {
        Bound::After(t) => ("oldest", t, true),
        Bound::Before(t) => ("latest", t, false),
        Bound::UpTo(t) => ("latest", t, true),
    };
    let at = at.to_string();
    let mut params = vec![("channel", channel), (key, at.as_str())];
    if inclusive {
        params.push(("inclusive", "true"));
    }
    params.extend([("limit", count.as_str()), ("ignore_replies", "true")]);
    client.call(&Call::form(Method::ConversationsHistory, &params), Priority::User).await
}

/// The times of the messages of `page` that have one.
fn times(page: &wire::History) -> Vec<u64> {
    page.messages.iter().flatten().filter_map(|m| wire::ts(&m.ts)).collect()
}
