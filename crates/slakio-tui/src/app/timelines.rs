//! The messages of each conversation and thread open in a pane, held once per [`Target`]
//! (every target names its workspace), whatever number of panes show it: two panes on one
//! conversation share its messages and load them once. Pages arrive newest first, then older
//! ones (sanitised on arrival into [`Shown`] messages); the page asked for and not answered yet
//! is recorded by its request id, so a late answer is told from the awaited one. Messages are
//! kept in timestamp order.
//!
//! Older pages are asked for one at a time and only when a gesture wants one (moving up near the
//! top, `gg`, `r`): one is queued, then sent when the pace allows ([`super::work`]). A page that
//! failed stays failed until retried (`r`, the next gesture that wants it, or by itself once
//! Slack's `Retry-After` passed).
//!
//! A timeline lives while a pane shows its target: the work area drops the others, and opening
//! the target again loads it afresh.

use super::pane::Shown;
use slakio_core::backend::{FailKind, Generation, Page};
use slakio_core::model::{Target, ThreadSummary, Ts};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long a page Slack asked to slow down for waits before it is asked for again by itself,
/// when Slack did not say.
pub const RETRY_PAUSE: Duration = Duration::from_secs(30);

/// The longest pause before a failed catch-up is asked for again by itself (the pause doubles
/// from [`RETRY_PAUSE`] each time it fails again).
pub const RECATCH_MAX: Duration = Duration::from_secs(15 * 60);

/// Slack's error codes that may pass by themselves: a catch-up refused with one is asked for
/// again; one refused with any other code is not.
pub const PASSING: [&str; 4] = ["internal_error", "fatal_error", "service_unavailable", "request_timeout"];

#[derive(Clone, Debug, Default)]
pub struct Timeline {
    /// Oldest first.
    pub items: Vec<Shown>,
    /// The oldest message is loaded.
    pub complete: bool,
    /// A page was asked for and has not arrived; its request id.
    pub pending: Option<Generation>,
    /// The last page asked for failed, and why: no more are asked for (until asked to retry), and
    /// an empty timeline says so.
    pub failed: Option<FailKind>,
    /// The messages are the local cache's, from an earlier run: the page awaited replaces them,
    /// and a retry asks for the newest page again (not an older one).
    pub cached: bool,
    /// A thread read only in part: its newest replies are not among those shown.
    pub cut: bool,
    /// An older page is wanted and waits for its turn (requests are paced).
    pub queued: bool,
    /// When the failed page is asked for again by itself (Slack asked to slow down).
    pub retry_at: Option<Instant>,
    /// An older page arrived: the history was read back from its newest page.
    pub paged: bool,
    /// The newest page asked for again after live updates were lost; its request id.
    pub catchup: Option<Generation>,
    /// The newest message a page loaded (the cache's, or when a catch-up was asked): those newer
    /// came live after it, and the page that replaces what was loaded keeps them.
    pub live_after: Option<Ts>,
    /// A catch-up failed: when it is asked for again by itself (what it was to read is missing
    /// until then).
    pub recatch: Option<Instant>,
    /// Catch-ups that failed in a row.
    pub catchup_fails: u32,
}

impl Timeline {
    /// The local cache's newest messages (already sanitised) arrived before the awaited page:
    /// shown until it comes. Only into an empty timeline; it still waits.
    pub fn add_cached(&mut self, shown: Vec<Shown>) {
        if self.items.is_empty() {
            self.live_after = shown.last().map(|m| m.ts);
            self.items = shown;
            self.cached = true;
        }
    }

    /// A page arrived (already sanitised): put what is older than the loaded messages before
    /// them (an echo or a late page never doubles). How many were put there.
    pub fn add_page(&mut self, page: &Page, shown: Vec<Shown>) -> usize {
        self.pending = None;
        self.complete = page.complete;
        // The backend's page replaces what the cache had, but not what came live on top of it
        // and is newer than the page.
        let mut live = Vec::new();
        if std::mem::take(&mut self.cached) {
            let (after, newest) = (self.live_after.take(), shown.last().map(|m| m.ts));
            live = std::mem::take(&mut self.items);
            live.retain(|m| after.is_some_and(|a| m.ts > a) && newest.is_none_or(|n| m.ts > n));
            self.cut = false;
        }
        self.cut |= page.cut;
        self.paged |= !self.items.is_empty();
        let first = self.items.first().map(|m| m.ts);
        let older: Vec<Shown> = shown.into_iter().filter(|m| first.is_none_or(|f| m.ts < f)).collect();
        let n = older.len();
        self.items.splice(0..0, older);
        self.items.extend(live);
        n
    }

    /// An older page is on its way, or waits for its turn.
    pub fn loading_older(&self) -> bool {
        !self.items.is_empty() && !self.cached && (self.pending.is_some() || self.queued)
    }

    /// Why the older page asked for last failed, while it stays failed (`None` for a timeline
    /// that never loaded, or the cache's).
    pub fn older_failed(&self) -> Option<&FailKind> {
        self.failed.as_ref().filter(|_| !self.items.is_empty() && !self.cached && !self.loading_older())
    }

    /// The failed page may be asked for again now: for a reason that may pass, or an older page
    /// whatever stopped it (an older page is always safe to ask for again; Slack's own errors,
    /// such as `internal_error`, often pass) but the sign-in or a backend that changes nothing.
    /// Not while Slack's pause runs: the page is asked for again by itself once it passed.
    pub fn retryable(&self) -> bool {
        let older = self.older_failed().is_some();
        self.retry_at.is_none()
            && self
                .failed
                .as_ref()
                .is_some_and(|k| k.retryable() || (older && !matches!(k, FailKind::Auth | FailKind::ReadOnly)))
    }

    /// The failed page waits out Slack's pause, to be asked for again by itself.
    pub fn paused(&self) -> bool {
        self.failed.is_some() && self.retry_at.is_some()
    }

    /// A message that arrived live (already sanitised), put in its place by its timestamp or
    /// over the copy there. A new one goes in only among what is loaded — newer than the oldest
    /// loaded message, or anywhere once the oldest one is — so none lands in a span not read;
    /// nor while nothing is loaded (the page awaited brings it). `changed`: a new copy, which
    /// only replaces (keeping the thread's count of replies when the copy has none). `true`
    /// when the timeline changed.
    pub fn put_live(&mut self, mut shown: Shown, changed: bool) -> bool {
        match self.items.binary_search_by_key(&shown.ts, |m| m.ts) {
            Ok(i) => {
                if shown.thread.is_none() {
                    shown.thread = self.items[i].thread;
                }
                self.items[i] = shown;
                true
            }
            Err(i) if !changed && (i > 0 || self.complete) && (!self.items.is_empty() || self.pending.is_none()) => {
                self.items.insert(i, shown);
                true
            }
            Err(_) => false,
        }
    }

    /// The message at `ts` was deleted. `true` when it was loaded.
    pub fn remove(&mut self, ts: Ts) -> bool {
        let Some(i) = self.position(ts) else { return false };
        self.items.remove(i);
        true
    }

    /// Reply `reply` came under the message at `parent`: one more on its count of replies (once:
    /// a reply not newer than the last one counted is not counted again). `true` when changed.
    pub fn add_reply(&mut self, parent: Ts, reply: Ts) -> bool {
        let Some(i) = self.position(parent) else { return false };
        let m = &mut self.items[i];
        match &mut m.thread {
            Some(t) if reply <= t.last_reply => false,
            Some(t) => {
                (t.replies, t.last_reply) = (t.replies.saturating_add(1), reply);
                true
            }
            None => {
                m.thread = Some(ThreadSummary { replies: 1, last_reply: reply });
                true
            }
        }
    }

    /// A reply under the message at `parent` was deleted (and was loaded): one fewer on its count.
    pub fn drop_reply(&mut self, parent: Ts) -> bool {
        let Some(i) = self.position(parent) else { return false };
        let m = &mut self.items[i];
        let Some(t) = &mut m.thread else { return false };
        t.replies = t.replies.saturating_sub(1);
        if t.replies == 0 {
            m.thread = None;
        }
        true
    }

    /// The newest page is asked for again (request `id`) after live updates were lost: what is
    /// loaded now (the newest message from before) is what the page must reach back to.
    pub fn ask_catch_up(&mut self, id: Generation) {
        self.catchup = Some(id);
        self.recatch = None;
        if self.live_after.is_none() {
            self.live_after = self.items.last().map(|m| m.ts);
        }
    }

    /// The newest page read again after live updates were lost (already sanitised), in place of
    /// what was loaded over the span it covers: what it lacks there was deleted meanwhile. When it
    /// does not reach back to the newest message loaded when it was asked for, what came between
    /// is unknown: the page replaces everything (reading back pages the rest again), so no gap is
    /// ever hidden. What came live meanwhile and is newer than the page stays after it.
    pub fn catch_up(&mut self, page: &Page, shown: Vec<Shown>) {
        self.catchup = None;
        (self.recatch, self.catchup_fails) = (None, 0);
        let from = self.live_after.take().or_else(|| self.items.last().map(|m| m.ts));
        let newest = shown.last().map(|m| m.ts);
        let live = |m: &Shown| from.is_none_or(|f| m.ts > f) && newest.is_none_or(|n| m.ts > n);
        let Some(oldest) = shown.first().map(|m| m.ts) else {
            if page.complete {
                self.items.retain(live);
                self.complete = true;
            }
            return;
        };
        let tail: Vec<Shown> = self.items.iter().filter(|m| live(m)).cloned().collect();
        if page.complete {
            // The whole conversation: nothing older is left.
            (self.items, self.complete) = (shown, true);
        } else if from.is_some_and(|f| f >= oldest) {
            self.items.retain(|m| m.ts < oldest);
            self.items.extend(shown);
        } else {
            self.items = shown;
            (self.complete, self.paged) = (false, false);
        }
        self.items.extend(tail);
    }

    /// The index of the message at `ts`, when loaded.
    pub fn position(&self, ts: Ts) -> Option<usize> {
        self.items.binary_search_by_key(&ts, |m| m.ts).ok()
    }

    /// The index of the message at `ts` (binary search), or, if it is not there (any more), of
    /// the nearest newer one, else of the nearest older one (the newest); `None` while nothing
    /// is loaded. A selection whose message is deleted moves there.
    pub fn index_near(&self, ts: Ts) -> Option<usize> {
        let last = self.items.len().checked_sub(1)?;
        Some(self.items.binary_search_by_key(&ts, |m| m.ts).unwrap_or_else(|i| i.min(last)))
    }
}

#[derive(Clone, Debug, Default)]
pub struct TimelineStore {
    timelines: HashMap<Target, Timeline>,
}

impl TimelineStore {
    pub fn get(&self, target: &Target) -> Option<&Timeline> {
        self.timelines.get(target)
    }

    pub fn get_mut(&mut self, target: &Target) -> Option<&mut Timeline> {
        self.timelines.get_mut(target)
    }

    /// The timeline of `target`, empty if it had none.
    pub fn entry(&mut self, target: &Target) -> &mut Timeline {
        self.timelines.entry(target.clone()).or_default()
    }

    /// The timeline waiting for the answer `id` about `target`.
    pub fn awaiting(&mut self, target: &Target, id: Generation) -> Option<&mut Timeline> {
        self.timelines.get_mut(target).filter(|t| t.pending == Some(id))
    }

    /// The timeline whose catch-up is the answer `id` about `target`.
    pub fn caught_up(&mut self, target: &Target, id: Generation) -> Option<&mut Timeline> {
        self.timelines.get_mut(target).filter(|t| t.catchup == Some(id))
    }

    /// The page asked for as request `id` failed at `now`: its timeline stops waiting; when
    /// Slack asked to slow down, it asks again by itself once Slack's pause passed. The target,
    /// when one waited for it.
    pub fn fail(&mut self, id: Generation, kind: FailKind, now: Instant) -> Option<Target> {
        // A catch-up that failed leaves what was loaded as it was, and is asked for again by
        // itself after a pause that doubles each time (unless the sign-in failed, or Slack
        // refused it for good).
        if let Some((target, tl)) = self.timelines.iter_mut().find(|(_, t)| t.catchup == Some(id)) {
            tl.catchup = None;
            tl.catchup_fails = tl.catchup_fails.saturating_add(1);
            let pause = RETRY_PAUSE.saturating_mul(1 << (tl.catchup_fails - 1).min(6)).min(RECATCH_MAX);
            tl.recatch = match &kind {
                FailKind::Auth | FailKind::ReadOnly => None,
                // Slack refused it for good (the user left the channel, it is gone): asking
                // again changes nothing. Only its own errors that pass are asked again.
                FailKind::Api { code } if !PASSING.contains(&code.as_str()) => None,
                FailKind::RateLimited { retry_after: Some(after) } => Some(now + pause.max(*after)),
                _ => Some(now + pause),
            };
            return Some(target.clone());
        }
        let (target, tl) = self.timelines.iter_mut().find(|(_, t)| t.pending == Some(id))?;
        tl.pending = None;
        tl.retry_at = match &kind {
            FailKind::RateLimited { retry_after } => Some(now + retry_after.unwrap_or(RETRY_PAUSE)),
            _ => None,
        };
        tl.failed = Some(kind);
        Some(target.clone())
    }

    /// Every timeline, to look through.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Target, &mut Timeline)> {
        self.timelines.iter_mut()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Target, &Timeline)> {
        self.timelines.iter()
    }

    /// Keep only the timelines `keep` says yes to.
    pub fn retain(&mut self, mut keep: impl FnMut(&Target) -> bool) {
        self.timelines.retain(|t, _| keep(t));
    }
}

#[cfg(test)]
mod tests;
