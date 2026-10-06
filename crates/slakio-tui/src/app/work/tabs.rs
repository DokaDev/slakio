//! The work area's tabs: a new tab for a target, showing, stepping and moving tabs, naming
//! them, closing a tab and opening it again where it was. The order and the rules are
//! [`slakio_core::layout::tabs`]'s; here the panes of a tab come and go with it.
//!
//! A closed tab is kept as what its panes showed (their targets and histories, the thread
//! panel each opened), its layout and name and where it was, the last [`REOPEN`] of them for
//! this session. Opened again, it gets new panes in the same layout.

use super::Work;
use crate::app::pane::{History, Pane};
use slakio_core::layout::tabs::Tab;
use slakio_core::layout::{Node, PaneId};
use slakio_core::model::Target;

/// Closed tabs kept to open again.
pub const REOPEN: usize = 20;

/// A tab closed, as it is opened again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosedTab {
    /// Where it was.
    pub index: usize,
    pub name: Option<String>,
    /// Its layout, by the ids its panes had.
    root: Node,
    active: PaneId,
    /// Each pane: its old id, what it showed, its history and the thread panel it opened.
    panes: Vec<(PaneId, Target, History, Option<PaneId>)>,
}

impl ClosedTab {
    /// Drop the targets `exists` says are gone from the histories; `false` when a pane showed
    /// one (the tab cannot come back as it was).
    pub fn retain(&mut self, exists: impl Fn(&Target) -> bool) -> bool {
        for (_, _, history, _) in &mut self.panes {
            history.retain(&exists);
        }
        self.panes.iter().all(|(_, t, ..)| exists(t))
    }
}

impl Work {
    /// Open `target` in a new tab, right after the one shown; what is open already, in any tab,
    /// is focused instead. The pane to focus.
    pub fn open_tab(&mut self, target: Target) -> PaneId {
        if let Some(id) = self.showing(&target) {
            self.activate(id);
            return id;
        }
        self.new_tab(target, History::default())
    }

    /// Show tab `i`; its active pane, `None` when there is no such tab.
    pub fn select_tab(&mut self, i: usize) -> Option<PaneId> {
        let id = self.tabs.all().get(i)?.active;
        self.activate(id);
        Some(id)
    }

    /// Show the tab `by` places to the right (negative: left), round past either end; its active
    /// pane.
    pub fn step_tab(&mut self, by: isize) -> Option<PaneId> {
        let n = self.tabs.len() as isize;
        let at = self.tabs.current()? as isize;
        self.select_tab((at + by).rem_euclid(n.max(1)) as usize)
    }

    /// Move the tab shown `by` places (no wrapping); `false` when it cannot go further.
    pub fn shift_tab(&mut self, by: isize) -> bool {
        self.tabs.shift(by)
    }

    /// Move tab `from` to index `to` (dragged there).
    pub fn move_tab(&mut self, from: usize, to: usize) -> bool {
        self.tabs.move_tab(from, to)
    }

    /// Name the tab shown `name`; empty: named after its active pane again. `false` with no tab.
    pub fn rename(&mut self, name: &str) -> bool {
        let Some(tab) = self.tabs.current_tab_mut() else { return false };
        let name = name.trim();
        tab.name = (!name.is_empty()).then(|| name.to_string());
        true
    }

    /// Close tab `i` and its panes, keeping it to open again; the tab to its right is shown, or
    /// the new last one. The target of its active pane (for the list, when none is left).
    pub fn close_tab(&mut self, i: usize) -> Option<Target> {
        self.set_insert(false);
        let tab = self.tabs.close(i)?;
        let panes = tab
            .root
            .leaves()
            .into_iter()
            .filter_map(|id| self.panes.remove(&id).map(|p| (id, p.target, p.history, p.thread)))
            .collect::<Vec<_>>();
        let target = panes.iter().find(|(id, ..)| *id == tab.active).map(|(_, t, ..)| t.clone());
        self.closed.push(ClosedTab { index: i, name: tab.name, root: tab.root, active: tab.active, panes });
        if self.closed.len() > REOPEN {
            self.closed.remove(0);
        }
        self.prune();
        self.fill();
        self.check();
        target
    }

    /// Open the tab closed last again, where it was, with new panes; its active pane. What is
    /// open already, in any tab, stays out of it (never open twice): a tab with nothing else
    /// focuses that pane instead. `None` when no closed tab is kept.
    pub fn reopen(&mut self) -> Option<PaneId> {
        let c = self.closed.pop()?;
        self.set_insert(false);
        let (open, kept): (Vec<_>, Vec<_>) = c.panes.iter().partition(|(_, t, ..)| self.showing(t).is_some());
        let Some(root) = open.iter().try_fold(c.root.clone(), |root, (old, ..)| root.without(*old)) else {
            let target = c.panes.iter().find(|(id, ..)| *id == c.active).or(c.panes.first()).map(|(_, t, ..)| t)?;
            let id = self.showing(target)?;
            self.activate(id);
            return Some(id);
        };
        let mut ids = Vec::new();
        for (old, target, history, _) in &kept {
            ids.push((*old, self.add(Pane { history: history.clone(), ..Pane::new(target.clone()) })));
        }
        let new = |old: PaneId| ids.iter().find(|(o, _)| *o == old).map(|(_, n)| *n);
        for (old, _, _, thread) in &kept {
            if let Some(p) = new(*old).and_then(|id| self.panes.get_mut(&id)) {
                p.thread = thread.and_then(new);
            }
        }
        let root = root.map(&|old| new(old).unwrap_or(old));
        let active = new(c.active).unwrap_or_else(|| root.leaves()[0]);
        self.tabs.insert(c.index, Tab { root, active, name: c.name });
        self.prune();
        self.fill();
        self.check();
        Some(active)
    }

    /// Tabs closed and kept to open again.
    pub fn closed_tabs(&self) -> usize {
        self.closed.len()
    }
}
