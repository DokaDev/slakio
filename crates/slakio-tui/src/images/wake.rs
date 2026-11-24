//! The loop wakes when a failed picture (or a workspace's emoji list) may be asked again: a
//! frame then asks for what it shows, with no key pressed and nothing else drawn meanwhile.

use super::{Entry, Images};
use std::time::Instant;

impl Images {
    /// The next time a failed picture or emoji list may be asked again, after the last wake.
    pub fn retry_at(&self) -> Option<Instant> {
        let pictures = self.entries.values().filter_map(|e| match e {
            Entry::Failed { again, .. } => *again,
            _ => None,
        });
        (pictures.chain(self.customs.retry_times())).filter(|at| self.woken.is_none_or(|w| *at > w)).min()
    }

    /// The loop woke at `now`: `true` when a wait is over, so a frame asks again.
    pub fn woke(&mut self, now: Instant) -> bool {
        let over = self.retry_at().is_some_and(|at| at <= now);
        if over {
            self.woken = Some(now);
        }
        over
    }
}

#[cfg(test)]
mod tests;
