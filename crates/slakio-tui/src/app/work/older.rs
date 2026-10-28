//! The pace of older pages. Reading back through a long history costs a request a page, and a
//! company workspace's request budget matters more than reaching its first message fast: a
//! gesture wants one more page at most ([`super::Work::fill`] queues it), and the pages queued
//! go out one at a time, at least [`OLDER_GAP`] apart, whatever the gestures (holding `k`,
//! pressing `gg` again and again). A page Slack asked to slow down for is asked for again by
//! itself once Slack's pause passed.

use super::Work;
use crate::app::pane::PAGE;
use slakio_core::backend::Command;
use slakio_core::model::Target;
use std::time::{Duration, Instant};

/// Older pages are asked for at most this often (all panes together): under Slack's own pace
/// for reading a history (about 50 a minute), so paging back never runs into its limit.
pub const OLDER_GAP: Duration = Duration::from_millis(1500);

impl Work {
    /// When an older page may be asked for next, if one is queued; or a failed page is asked
    /// for again by itself.
    pub fn deadline(&self) -> Option<Instant> {
        let queued = self.timelines.iter().any(|(_, tl)| tl.queued);
        let next = self.older_at.map(|at| at + OLDER_GAP).filter(|_| queued);
        let retry = self.timelines.iter().filter_map(|(_, tl)| tl.retry_at.filter(|_| tl.failed.is_some())).min();
        next.into_iter().chain(retry).min()
    }

    /// At `now`: the failed pages whose pause passed are asked for again, and one queued older
    /// page goes out when its turn came (the active pane's first). `true` when the screen
    /// changes (a failed row turns into a loading one).
    pub fn send_older(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for (_, tl) in self.timelines.iter_mut() {
            if tl.failed.is_some() && tl.retry_at.is_some_and(|at| at <= now) {
                tl.queued = tl.older_failed().is_some();
                (tl.failed, tl.retry_at) = (None, None);
                changed = true;
            }
        }
        if changed {
            self.fill();
        }
        if self.older_at.is_some_and(|at| now < at + OLDER_GAP) {
            return changed;
        }
        let active = self.active().and_then(|id| self.panes.get(&id)).map(|p| p.target.clone());
        let queued = |t: &Target| self.timelines.get(t).is_some_and(|tl| tl.queued);
        let Some(target) =
            active.filter(queued).or_else(|| self.timelines.iter().find(|(_, tl)| tl.queued).map(|(t, _)| t.clone()))
        else {
            return changed;
        };
        let tl = self.timelines.entry(&target);
        tl.queued = false;
        let Some(before) = tl.items.first().map(|m| m.ts).filter(|_| !tl.complete) else { return true };
        let command = Command::History { target: target.clone(), before: Some(before), limit: PAGE };
        tl.pending = Some(self.requests.ask(Some(target.workspace().clone()), command));
        self.older_at = Some(now);
        true
    }
}
