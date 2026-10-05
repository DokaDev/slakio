//! The shell: which region has the focus (rail, list panel, work area), the rail's cursor, the
//! view the list panel shows, the list's cursor and scroll, folded sections, and what the work
//! area has open. It owns its update ([`Shell::update`]); the rows come from the read model.
//!
//! ```text
//! rail          list panel          work area
//! ▌A  ▌B        ▾ Favorites         #backend
//! ──            # backend   3
//! H D A F L     # incidents ●
//! ```

use super::model::{Model, Row};
use crate::action::ShellAction;
use slakio_core::model::{SectionId, Target};
use std::collections::HashSet;

/// The regions of the main screen, left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Rail,
    List,
    Work,
}

/// What the list panel shows, picked on the rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum View {
    Home,
    Dms,
    Activity,
    Files,
    Later,
}

impl View {
    /// In the rail's order.
    pub const ALL: &'static [View] = &[View::Home, View::Dms, View::Activity, View::Files, View::Later];

    /// Built in a later version: the list panel shows a placeholder, never invented data that
    /// could pass for the real thing.
    pub fn is_placeholder(self) -> bool {
        matches!(self, View::Activity | View::Files | View::Later)
    }
}

/// One item of the rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailItem {
    /// A workspace (index into the model's workspaces).
    Workspace(usize),
    View(View),
}

/// The rail's items for `workspaces` workspaces: the workspaces, then the views.
pub fn rail_items(workspaces: usize) -> Vec<RailItem> {
    (0..workspaces).map(RailItem::Workspace).chain(View::ALL.iter().map(|v| RailItem::View(*v))).collect()
}

#[derive(Clone, Debug)]
pub struct Shell {
    pub focus: Region,
    /// Index into [`rail_items`].
    pub rail_cursor: usize,
    /// The workspace the list panel shows.
    pub workspace: usize,
    pub view: View,
    /// Index into the list's rows.
    pub list_cursor: usize,
    /// The first row on screen.
    pub list_top: usize,
    pub collapsed: HashSet<SectionId>,
    pub list_hidden: bool,
    /// The mouse is over the rail.
    pub hover_rail: bool,
    /// What the work area shows.
    pub open: Option<Target>,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            focus: Region::List,
            rail_cursor: 0,
            workspace: 0,
            view: View::Home,
            list_cursor: 0,
            list_top: 0,
            collapsed: HashSet::new(),
            list_hidden: false,
            hover_rail: false,
            open: None,
        }
    }
}

impl Shell {
    /// The rail shows labels: it has the focus or the mouse.
    pub fn rail_expanded(&self) -> bool {
        self.focus == Region::Rail || self.hover_rail
    }

    pub fn rows(&self, model: &Model) -> Vec<Row> {
        model.rows(self.workspace, self.view, &self.collapsed)
    }

    /// Apply `action`. `list_height` is the number of rows the list panel shows (for scrolling).
    pub fn update(&mut self, action: ShellAction, model: &Model, list_height: usize) {
        let items = rail_items(model.workspaces().len());
        let rows = self.rows(model).len();
        match action {
            ShellAction::FocusLeft => self.focus = self.neighbour(-1),
            ShellAction::FocusRight => self.focus = self.neighbour(1),
            ShellAction::RailNext => self.rail_cursor = (self.rail_cursor + 1).min(items.len().saturating_sub(1)),
            ShellAction::RailPrev => self.rail_cursor = self.rail_cursor.saturating_sub(1),
            ShellAction::RailSelect => {
                if let Some(item) = items.get(self.rail_cursor).copied() {
                    self.select(item, &items);
                }
            }
            ShellAction::ListNext => self.list_cursor = (self.list_cursor + 1).min(rows.saturating_sub(1)),
            ShellAction::ListPrev => self.list_cursor = self.list_cursor.saturating_sub(1),
            ShellAction::ListFirst => self.list_cursor = 0,
            ShellAction::ListLast => self.list_cursor = rows.saturating_sub(1),
            ShellAction::ListOpen => self.open_row(model),
            ShellAction::ToggleList => {
                self.list_hidden = !self.list_hidden;
                if self.list_hidden && self.focus == Region::List {
                    self.focus = Region::Work;
                }
            }
            ShellAction::Show(view) => self.select(RailItem::View(view), &items),
        }
        self.scroll(list_height);
    }

    /// Show `item` (one of `items`, the rail) in the list panel and move the focus there.
    pub fn select(&mut self, item: RailItem, items: &[RailItem]) {
        match item {
            RailItem::Workspace(ws) => {
                self.workspace = ws;
                self.view = View::Home;
            }
            RailItem::View(v) => self.view = v,
        }
        self.rail_cursor = items.iter().position(|i| *i == item).unwrap_or(self.rail_cursor);
        self.list_cursor = 0;
        self.list_top = 0;
        self.list_hidden = false;
        self.focus = Region::List;
    }

    /// Open the conversation under the cursor, or fold or unfold its section.
    pub fn open_row(&mut self, model: &Model) {
        match self.rows(model).get(self.list_cursor) {
            Some(Row::Section(i)) => {
                let id = model.section(*i).id.clone();
                if !self.collapsed.remove(&id) {
                    self.collapsed.insert(id);
                }
            }
            Some(Row::Conversation(i)) => {
                let c = model.conversation(*i);
                self.open = Some(Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() });
            }
            None => {}
        }
    }

    /// Keep the list cursor on screen.
    pub fn scroll(&mut self, height: usize) {
        let height = height.max(1);
        if self.list_cursor < self.list_top {
            self.list_top = self.list_cursor;
        } else if self.list_cursor >= self.list_top + height {
            self.list_top = self.list_cursor + 1 - height;
        }
    }

    /// Keep the cursors inside what the model holds (after a new boot answer).
    pub fn clamp(&mut self, model: &Model, list_height: usize) {
        let n = model.workspaces().len();
        self.workspace = self.workspace.min(n.saturating_sub(1));
        self.rail_cursor = self.rail_cursor.min(rail_items(n).len() - 1);
        self.list_cursor = self.list_cursor.min(self.rows(model).len().saturating_sub(1));
        if self.open.as_ref().is_some_and(|t| model.target(t).is_none()) {
            self.open = None;
        }
        self.scroll(list_height);
    }

    /// The region `step` places to the left (-1) or right (1), skipping a hidden list panel.
    fn neighbour(&self, step: i8) -> Region {
        let order: Vec<Region> = [Region::Rail, Region::List, Region::Work]
            .into_iter()
            .filter(|r| *r != Region::List || !self.list_hidden)
            .collect();
        let at = order.iter().position(|r| *r == self.focus).unwrap_or(0);
        let to = (at as i8 + step).clamp(0, order.len() as i8 - 1);
        order[to as usize]
    }
}

#[cfg(test)]
mod tests;
