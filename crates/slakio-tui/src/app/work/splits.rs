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
use crate::screen::{self, THREAD_SHARE, split_share};
use ratatui::layout::Rect;
use slakio_core::layout::{Dir, Node, PaneId, Seam};
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

    /// A pane of the tab shown is some pane's auto thread panel.
    pub fn has_panel(&self) -> bool {
        self.ids().into_iter().any(|id| self.owner(id).is_some())
    }

    /// Open `target` in a new pane after pane `at` (and its thread panel) along `dir`, the two
    /// sharing their room evenly; the new pane, which is the tab's active one. Beside, the
    /// thread panel of `at` would have no room left to show: it splits off as a pane of its own
    /// first, and the three share the room.
    pub fn split(&mut self, at: PaneId, dir: Dir, target: Target) -> Option<PaneId> {
        self.tabs.find(at)?;
        self.set_insert(false);
        let (unit, share) = match self.panes.get(&at).and_then(|p| p.thread).filter(|_| dir == Dir::Row) {
            Some(panel) => (vec![at, self.detach(panel, dir)?], split_share(dir, 2, 1)),
            None => (self.unit(at), split_share(dir, 1, 1)),
        };
        let id = self.add(Pane::new(target));
        if !self.tabs.split(&unit, dir, share, id) {
            self.panes.remove(&id);
            return None;
        }
        self.fit();
        self.fill();
        self.check();
        Some(id)
    }

    /// Split thread panel `panel` off as a pane of its own, after the pane that opened it along
    /// `dir`; the pane, which is the tab's active one. `None` when it is no pane's panel (or
    /// the split could not be made: the tab is left as it was).
    pub fn detach(&mut self, panel: PaneId, dir: Dir) -> Option<PaneId> {
        let owner = self.owner(panel)?;
        self.set_insert(false);
        let i = self.tabs.find(panel)?;
        let tab = self.tabs.get_mut(i)?;
        let before = (tab.root.clone(), tab.active);
        tab.root = tab.root.without(panel)?;
        tab.active = owner;
        if !self.tabs.split(&[owner], dir, split_share(dir, 1, 1), panel) {
            if let Some(tab) = self.tabs.get_mut(i) {
                (tab.root, tab.active) = before;
            }
            return None;
        }
        if let Some(p) = self.panes.get_mut(&owner) {
            p.thread = None;
        }
        self.fit();
        self.check();
        Some(panel)
    }

    /// Zoom the tab shown on its active pane, or show every pane again; whether it is zoomed
    /// now. `None` with no tab, or one of one pane.
    pub fn toggle_zoom(&mut self) -> Option<bool> {
        self.tabs.toggle_zoom()
    }

    /// The panes of the tab shown share its room evenly (a thread panel gets its first share
    /// back beside its pane). `Some(false)` when there is nothing to even: one pane (with its
    /// thread panel, or not); `None` with no tab.
    pub fn equalize(&mut self) -> Option<bool> {
        let panes = self.ids().into_iter().filter(|id| self.owner(*id).is_none()).count();
        let tab = self.tabs.current_tab_mut()?;
        if panes < 2 {
            return Some(false);
        }
        let panel = |s: &slakio_core::layout::Share| screen::is_thread_share(s).then_some(THREAD_SHARE);
        tab.root = screen::fitted(&tab.root.equalized(&panel, &split_share));
        Some(true)
    }

    /// Every tab's splits need the room their sides hold ([`screen::fitted`]): a smaller screen
    /// takes from every pane alike before a side gives way.
    pub(super) fn fit(&mut self) {
        for i in 0..self.tabs.len() {
            if let Some(t) = self.tabs.get_mut(i) {
                t.root = screen::fitted(&t.root);
            }
        }
    }

    /// The active pane `by` cells bigger along `dir` (smaller below zero) in the work area
    /// `work`, as far as every pane keeps its least; `false` when nothing changed (no split that
    /// way around it, at the end of its room, or the tab is zoomed).
    pub fn resize(&mut self, work: Rect, dir: Dir, by: i32) -> bool {
        let (Some(active), false) = (self.active(), self.zoomed()) else { return false };
        let Some(tab) = self.tabs.current_tab_mut() else { return false };
        let Some(root) = tab.root.resize(screen::work_area(work), Some(active), active, dir, by) else { return false };
        tab.root = root;
        true
    }

    /// The seams between the panes of the tab shown, laid out in `work`.
    pub fn seams(&self, work: Rect) -> Vec<Seam> {
        let active = self.active();
        self.shown_layout().map(|t| t.seams(screen::work_area(work), active)).unwrap_or_default()
    }

    /// Move seam `id` ([`Seam::id`]) to column (row) `at` of the work area `work`; `false` when
    /// it did not move.
    pub fn drag(&mut self, work: Rect, id: PaneId, at: u16) -> bool {
        let active = self.active();
        if self.zoomed() {
            return false;
        }
        let Some(tab) = self.tabs.current_tab_mut() else { return false };
        let Some(root) = tab.root.drag(screen::work_area(work), active, id, at) else { return false };
        tab.root = root;
        true
    }

    /// The pane the keyboard goes to when `id` closes: of the panes that take its room (the
    /// other side of the split it is a side of, with its thread panel), the one the keyboard was
    /// on last, a thread panel only where there is no other pane.
    pub(super) fn near(&self, id: PaneId) -> Option<PaneId> {
        let root = &self.tabs.all().get(self.tabs.find(id)?)?.root;
        let side = root.sibling(root.around(&self.unit(id))?)?.leaves();
        let last = |ids: &mut dyn Iterator<Item = &PaneId>| {
            ids.min_by_key(|p| std::cmp::Reverse(self.panes.get(p).map_or(0, |p| p.focused_at))).copied()
        };
        last(&mut side.iter().filter(|p| self.owner(**p).is_none())).or_else(|| last(&mut side.iter()))
    }
}
