//! The work area's tabs as saved between runs ([`slakio_core::layout::saved`]) and opened
//! again: the same tabs in the same order with the same names, layouts and shares, each pane
//! showing what it showed (the thread panels beside the panes that opened them), the tab shown,
//! its active pane and its zoom. A pane is a new view: its selection, history and draft start
//! afresh (drafts are not saved).

use super::Work;
use crate::app::pane::Pane;
use slakio_core::layout::saved::{Saved, SavedPane, SavedTab};
use slakio_core::layout::tabs::Tab;
use slakio_core::layout::{Node, PaneId};
use slakio_core::model::Target;

impl Work {
    /// The tabs as they are now, to save.
    pub fn saved(&self) -> Saved {
        let tabs = self
            .tabs
            .all()
            .iter()
            .map(|t| {
                let ids = t.root.leaves();
                let index = |id: PaneId| PaneId(ids.iter().position(|p| *p == id).unwrap_or(0) as u64);
                let panes = ids
                    .iter()
                    .filter_map(|id| self.panes.get(id))
                    .map(|p| SavedPane { target: p.target.clone(), panel: p.thread.map(index) })
                    .collect();
                SavedTab {
                    name: t.name.clone(),
                    root: t.root.map(&index),
                    active: index(t.active),
                    zoomed: t.zoomed,
                    panes,
                }
            })
            .collect();
        Saved { tabs, current: self.tabs.current().unwrap_or(0) }
    }

    /// Open the tabs `saved` after those open (none, at the start), leaving out the panes whose
    /// target `keep` refuses (a tab left with none is left out); the tab saved as shown is
    /// shown, on its active pane. How many panes were left out.
    pub fn restore(&mut self, saved: &Saved, keep: impl Fn(&Target) -> bool) -> usize {
        let (mut dropped, mut shown) = (0, None);
        for (i, t) in saved.tabs.iter().enumerate() {
            let gone: Vec<PaneId> =
                (0..t.panes.len() as u64).map(PaneId).filter(|p| !keep(&t.panes[p.0 as usize].target)).collect();
            dropped += gone.len();
            let Some(root) = gone.iter().try_fold(t.root.clone(), |root, p| root.without(*p)) else { continue };
            let new: Vec<Option<PaneId>> = t
                .panes
                .iter()
                .enumerate()
                .map(|(k, p)| (!gone.contains(&PaneId(k as u64))).then(|| self.add(Pane::new(p.target.clone()))))
                .collect();
            let to = |old: PaneId| new.get(old.0 as usize).copied().flatten();
            for (k, p) in t.panes.iter().enumerate() {
                if let Some(owner) = new[k].and_then(|id| self.panes.get_mut(&id)) {
                    owner.thread = p.panel.and_then(to);
                }
            }
            let root = root.map(&|old| to(old).unwrap_or(old));
            let active = to(t.active).unwrap_or_else(|| root.leaves()[0]);
            let zoomed = t.zoomed && matches!(root, Node::Split { .. });
            let at = self.tabs.insert(self.tabs.len(), Tab { root, active, name: t.name.clone(), zoomed });
            if i == saved.current || shown.is_none() {
                shown = Some(at);
            }
        }
        self.fit();
        // Shown on its active pane, zoomed as it was.
        if let Some(active) = shown.and_then(|i| self.tabs.all().get(i)).map(|t| t.active) {
            self.point(active);
        }
        self.prune();
        self.fill();
        self.check();
        dropped
    }
}
