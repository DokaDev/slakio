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
use slakio_core::model::{Target, Ts};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long a page Slack asked to slow down for waits before it is asked for again by itself,
/// when Slack did not say.
pub const RETRY_PAUSE: Duration = Duration::from_secs(30);

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
}

impl Timeline {
    /// The local cache's newest messages (already sanitised) arrived before the awaited page:
    /// shown until it comes. Only into an empty timeline; it still waits.
    pub fn add_cached(&mut self, shown: Vec<Shown>) {
        if self.items.is_empty() {
            self.items = shown;
            self.cached = true;
        }
    }

    /// A page arrived (already sanitised): put what is older than the loaded messages before
    /// them (an echo or a late page never doubles). How many were put there.
    pub fn add_page(&mut self, page: &Page, shown: Vec<Shown>) -> usize {
        self.pending = None;
        self.complete = page.complete;
        // The backend's page replaces what the cache had.
        if std::mem::take(&mut self.cached) {
            self.items.clear();
            self.cut = false;
        }
        self.cut |= page.cut;
        self.paged |= !self.items.is_empty();
        let first = self.items.first().map(|m| m.ts);
        let older: Vec<Shown> = shown.into_iter().filter(|m| first.is_none_or(|f| m.ts < f)).collect();
        let n = older.len();
        self.items.splice(0..0, older);
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

    /// The timeline of `target`, empty if it had none.
    pub fn entry(&mut self, target: &Target) -> &mut Timeline {
        self.timelines.entry(target.clone()).or_default()
    }

    /// The timeline waiting for the answer `id` about `target`.
    pub fn awaiting(&mut self, target: &Target, id: Generation) -> Option<&mut Timeline> {
        self.timelines.get_mut(target).filter(|t| t.pending == Some(id))
    }

    /// The page asked for as request `id` failed at `now`: its timeline stops waiting; when
    /// Slack asked to slow down, it asks again by itself once Slack's pause passed. The target,
    /// when one waited for it.
    pub fn fail(&mut self, id: Generation, kind: FailKind, now: Instant) -> Option<Target> {
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
