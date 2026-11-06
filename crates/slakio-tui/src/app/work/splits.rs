//! The user's splits of a tab: a pane opened beside or under another, the thread panel split
//! off as a pane of its own, a pane zoomed to the whole tab and back, even room for all.
//!
//! ```text
//! ╭ #backend ──────╮╭ ⤷ Thread ──────╮      v: beside   s: under
//! │                ││                │
//! │                │├ @Minsu ────────┤
//! │                ││                │
//! ```
//!
//! What a split opens goes after the pane split, its thread panel (if it opened one) kept beside
//! it: the two are split as one. A thread panel split off stops being the panel: it stays a pane
//! with its messages, selection and history, and the next thread its pane opens comes in a new
//! panel. Each pane keeps its own selection, view and back/forward history; a target open in
//! two panes shares its messages and its draft.

use super::Work;
use crate::app::pane::Pane;
use crate::screen::{THREAD_SHARE, split_share};
use slakio_core::layout::{Dir, Node, PaneId};
use slakio_core::model::Target;
use std::borrow::Cow;

impl Work {
    /// The layout of the tab shown as drawn: its active pane alone while it is zoomed.
    pub fn shown_layout(&self) -> Option<Cow<'_, Node>> {
        self.tabs.current_tab().map(|t| t.shown())
    }

    /// The tab shown is zoomed on its active pane.
    pub fn zoomed(&self) -> bool {
        self.tabs.current_tab().is_some_and(|t| t.zoomed)
    }

    /// Pane `id` and the thread panel it opened, if any: what a split of it splits.
    pub fn unit(&self, id: PaneId) -> Vec<PaneId> {
        let panel = self.panes.get(&id).and_then(|p| p.thread);
        std::iter::once(id).chain(panel).collect()
    }

    /// Open `target` in a new pane after pane `at` (and its thread panel) along `dir`, the two
    /// sharing their room evenly; the new pane, which is the tab's active one.
    pub fn split(&mut self, at: PaneId, dir: Dir, target: Target) -> Option<PaneId> {
        self.tabs.find(at)?;
        self.set_insert(false);
        let unit = self.unit(at);
        let id = self.add(Pane::new(target));
        if !self.tabs.split(&unit, dir, split_share(dir, 1, 1), id) {
            self.panes.remove(&id);
            return None;
        }
        self.fill();
        self.check();
        Some(id)
    }

    /// Split thread panel `panel` off as a pane of its own, after the pane that opened it along
    /// `dir`; the pane, which is the tab's active one. `None` when it is no pane's panel.
    pub fn detach(&mut self, panel: PaneId, dir: Dir) -> Option<PaneId> {
        let owner = self.owner(panel)?;
        self.set_insert(false);
        let i = self.tabs.find(panel)?;
        let tab = self.tabs.get_mut(i)?;
        tab.root = tab.root.without(panel)?;
        tab.active = owner;
        if let Some(p) = self.panes.get_mut(&owner) {
            p.thread = None;
        }
        self.tabs.split(&[owner], dir, split_share(dir, 1, 1), panel);
        self.check();
        Some(panel)
    }

    /// Zoom the tab shown on its active pane, or show every pane again; whether it is zoomed
    /// now. `None` with no tab, or one of one pane.
    pub fn toggle_zoom(&mut self) -> Option<bool> {
        self.tabs.toggle_zoom()
    }

    /// The panes of the tab shown share its room evenly (a thread panel keeps its share beside
    /// its pane). `false` when the tab has one pane, or there is none.
    pub fn equalize(&mut self) -> bool {
        let Some(tab) = self.tabs.current_tab_mut().filter(|t| matches!(t.root, Node::Split { .. })) else {
            return false;
        };
        tab.root = tab.root.equalized(&|s| *s == THREAD_SHARE, &split_share);
        true
    }

    /// The pane the keyboard goes to when `id` closes: the one before it in reading order (not
    /// its own thread panel), else the one after it.
    pub(super) fn near(&self, id: PaneId) -> Option<PaneId> {
        let panel = self.panes.get(&id).and_then(|p| p.thread);
        let ids: Vec<PaneId> = self.ids().into_iter().filter(|p| Some(*p) != panel).collect();
        let at = ids.iter().position(|p| *p == id)?;
        at.checked_sub(1).or(Some(at + 1)).and_then(|i| ids.get(i)).copied()
    }
}
