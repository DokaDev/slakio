//! The messages of each conversation and thread open in a pane, held once per [`Target`]
//! (every target names its workspace), whatever number of panes show it: two panes on one
//! conversation share its messages and load them once. Pages arrive newest first, then older
//! ones (sanitised on arrival into [`Shown`] messages); the page asked for and not answered yet
//! is recorded by its request id, so a late answer is told from the awaited one.
//!
//! A timeline lives while a pane shows its target: the work area drops the others, and opening
//! the target again loads it afresh.

use super::pane::Shown;
use slakio_core::backend::{Generation, Page};
use slakio_core::model::{Target, Ts};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Timeline {
    /// Oldest first.
    pub items: Vec<Shown>,
    /// The oldest message is loaded.
    pub complete: bool,
    /// A page was asked for and has not arrived; its request id.
    pub pending: Option<Generation>,
}

impl Timeline {
    /// A page arrived (already sanitised): put what is older than the loaded messages before
    /// them (an echo or a late page never doubles). How many were put there.
    pub fn add_page(&mut self, page: &Page, shown: Vec<Shown>) -> usize {
        self.pending = None;
        self.complete = page.complete;
        let first = self.items.first().map(|m| m.ts);
        let older: Vec<Shown> = shown.into_iter().filter(|m| first.is_none_or(|f| m.ts < f)).collect();
        let n = older.len();
        self.items.splice(0..0, older);
        n
    }

    /// The index of the message at `ts`, when loaded.
    pub fn position(&self, ts: Ts) -> Option<usize> {
        self.items.iter().position(|m| m.ts == ts)
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

    /// Keep only the timelines `keep` says yes to.
    pub fn retain(&mut self, mut keep: impl FnMut(&Target) -> bool) {
        self.timelines.retain(|t, _| keep(t));
    }
}

#[cfg(test)]
mod tests;
