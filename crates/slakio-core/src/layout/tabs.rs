//! Tabs: each a named layout of panes ([`Node`]) and the pane of it the keyboard goes to, in
//! the order the user gave them. The app never reorders tabs: only the user's moves
//! ([`Tabs::shift`], [`Tabs::move_tab`]) change the order, and opening, closing and reopening a
//! tab leave the other tabs where they were.
//!
//! ```text
//!  1 #backend ×  2 ⤷ Deploy rollback ●3 ×  3 @Minsu ×
//! ```
//!
//! No tab is ever empty: closing the last pane of a tab closes the tab, and closing the last
//! tab leaves none (the work area is empty; nothing quits). A pane is in one tab at most.

use super::{Node, PaneId};

/// A tab: its panes' layout, the pane of it the keyboard goes to, and a name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub root: Node,
    /// One of the panes of `root`.
    pub active: PaneId,
    /// The name the user gave it; `None`: it is named after its active pane.
    pub name: Option<String>,
}

impl Tab {
    /// A tab of one pane.
    pub fn new(pane: PaneId) -> Self {
        Self { root: Node::Leaf(pane), active: pane, name: None }
    }
}

/// The tabs in their order, and the one shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tabs {
    tabs: Vec<Tab>,
    /// Index of the tab shown (any value while there is none).
    current: usize,
}

/// What closing a pane did to its tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Closed {
    /// The pane was in no tab.
    Nothing,
    /// The tab at this index keeps its other panes.
    Pane(usize),
    /// It was the tab's last pane: the tab, which was at this index, is closed.
    Tab(usize, Tab),
}

impl Tabs {
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Every tab, in order.
    pub fn all(&self) -> &[Tab] {
        &self.tabs
    }

    /// The index of the tab shown; `None` when there is none.
    pub fn current(&self) -> Option<usize> {
        (!self.tabs.is_empty()).then_some(self.current)
    }

    pub fn current_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.current)
    }

    pub fn current_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.current)
    }

    pub fn get_mut(&mut self, i: usize) -> Option<&mut Tab> {
        self.tabs.get_mut(i)
    }

    /// The tab pane `pane` is in.
    pub fn find(&self, pane: PaneId) -> Option<usize> {
        self.tabs.iter().position(|t| t.root.contains(pane))
    }

    /// Open `tab` right after the one shown (first when there is none); it is shown. Its index.
    pub fn open(&mut self, tab: Tab) -> usize {
        let at = if self.tabs.is_empty() { 0 } else { self.current + 1 };
        self.insert(at, tab)
    }

    /// Put `tab` at `at` (past the end: last), where a closed tab was; it is shown. Its index.
    pub fn insert(&mut self, at: usize, tab: Tab) -> usize {
        let at = at.min(self.tabs.len());
        self.tabs.insert(at, tab);
        self.current = at;
        at
    }

    /// Show tab `i`; `false` when there is no such tab.
    pub fn select(&mut self, i: usize) -> bool {
        if i >= self.tabs.len() {
            return false;
        }
        self.current = i;
        true
    }

    /// Show the tab `by` places to the right (negative: left), round past either end.
    pub fn step(&mut self, by: isize) {
        let n = self.tabs.len() as isize;
        if n > 0 {
            self.current = (self.current as isize + by).rem_euclid(n) as usize;
        }
    }

    /// Move the tab shown `by` places (the user's move; no wrapping). `false` when it cannot go.
    pub fn shift(&mut self, by: isize) -> bool {
        let Some(from) = self.current() else { return false };
        let to = from as isize + by;
        if to < 0 || to >= self.tabs.len() as isize {
            return false;
        }
        self.move_tab(from, to as usize)
    }

    /// Move tab `from` to index `to` (dragged there); the tab shown stays shown. `false` when
    /// either index is out of range or they are the same.
    pub fn move_tab(&mut self, from: usize, to: usize) -> bool {
        let n = self.tabs.len();
        if from >= n || to >= n || from == to {
            return false;
        }
        let shown = self.current;
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.current = match shown {
            s if s == from => to,
            s if from < s && s <= to => s - 1,
            s if to <= s && s < from => s + 1,
            s => s,
        };
        true
    }

    /// Close tab `i`; the tab to its right is shown, or the new last one. The tab closed.
    pub fn close(&mut self, i: usize) -> Option<Tab> {
        if i >= self.tabs.len() {
            return None;
        }
        let tab = self.tabs.remove(i);
        if self.current > i || self.current >= self.tabs.len() {
            self.current = self.current.saturating_sub(1);
        }
        Some(tab)
    }

    /// Take pane `pane` out of its tab, which closes when it was its last pane. Where the tab's
    /// active pane was `pane`, `next` (if it is still in the tab) or the tab's first pane is.
    pub fn close_pane(&mut self, pane: PaneId, next: Option<PaneId>) -> Closed {
        let Some(i) = self.find(pane) else { return Closed::Nothing };
        let tab = &mut self.tabs[i];
        match tab.root.without(pane) {
            Some(root) => {
                if tab.active == pane {
                    tab.active = next.filter(|n| root.contains(*n)).unwrap_or_else(|| root.leaves()[0]);
                }
                tab.root = root;
                Closed::Pane(i)
            }
            None => match self.close(i) {
                Some(tab) => Closed::Tab(i, tab),
                None => Closed::Nothing,
            },
        }
    }

    /// Every pane of every tab.
    pub fn panes(&self) -> Vec<PaneId> {
        self.tabs.iter().flat_map(|t| t.root.leaves()).collect()
    }

    /// What always holds (for tests and debug checks): every tab's active pane is one of its
    /// panes, a pane is in one tab only, and a tab is shown while there is one.
    pub fn holds(&self) -> bool {
        let panes = self.panes();
        let mut unique = panes.clone();
        unique.sort();
        unique.dedup();
        self.tabs.iter().all(|t| t.root.contains(t.active))
            && unique.len() == panes.len()
            && (self.tabs.is_empty() || self.current < self.tabs.len())
    }
}

#[cfg(test)]
mod tests;
