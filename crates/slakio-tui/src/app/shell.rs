//! The shell: which region has the focus (rail, list panel, work area), the rail's cursor, the
//! view the list panel shows, the list's cursor and scroll, and folded sections. It owns its
//! update ([`Shell::update`]), which hands back a conversation to open (the work area opens
//! it); the rows come from the read model.
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

/// A conversation to open from the list: `focus` moves the keyboard to it (`Enter`), else the
/// list keeps it (`l`, a peek).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    pub target: Target,
    pub focus: bool,
}
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
    /// The rail item under the mouse (index into [`rail_items`]).
    pub hover_item: Option<usize>,
    /// The list cursor goes to the first conversation once the rows are known.
    pending_home: bool,
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
            hover_item: None,
            pending_home: true,
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

    /// Apply `action`. `list_height` is the number of rows the list panel shows (for scrolling
    /// and paging). The conversation to open, when the action opens one.
    pub fn update(&mut self, action: ShellAction, model: &Model, list_height: usize) -> Option<Open> {
        let mut open = None;
        let items = rail_items(model.workspaces().len());
        let list = self.rows(model);
        let rows = list.len();
        let last_item = items.len().saturating_sub(1);
        let page = list_height.max(1) as isize;
        match action {
            ShellAction::FocusLeft => self.focus = self.neighbour(-1),
            ShellAction::FocusRight => self.focus = self.neighbour(1),
            // Up, down, next and previous need the work area: the app moves those.
            ShellAction::FocusUp | ShellAction::FocusDown | ShellAction::FocusNext | ShellAction::FocusPrev => {}
            ShellAction::FocusRail if self.focus == Region::Rail => self.focus = self.leave_rail(),
            ShellAction::FocusRail => self.focus = Region::Rail,
            ShellAction::RailNext => self.rail_cursor = (self.rail_cursor + 1).min(last_item),
            ShellAction::RailPrev => self.rail_cursor = self.rail_cursor.saturating_sub(1),
            ShellAction::RailFirst => self.rail_cursor = 0,
            ShellAction::RailLast => self.rail_cursor = last_item,
            ShellAction::RailSelect => {
                if let Some(item) = items.get(self.rail_cursor).copied() {
                    self.select(item, &items, model);
                }
            }
            ShellAction::RailLeave => self.focus = self.leave_rail(),
            ShellAction::ListNext => self.list_cursor = step(&list, self.list_cursor, 1),
            ShellAction::ListPrev => self.list_cursor = step(&list, self.list_cursor, -1),
            ShellAction::ListHalfDown => self.list_cursor = step(&list, self.list_cursor, (page / 2).max(1)),
            ShellAction::ListHalfUp => self.list_cursor = step(&list, self.list_cursor, -(page / 2).max(1)),
            ShellAction::ListPageDown => self.list_cursor = step(&list, self.list_cursor, page),
            ShellAction::ListPageUp => self.list_cursor = step(&list, self.list_cursor, -page),
            ShellAction::ListFirst => self.list_cursor = 0,
            ShellAction::ListLast => self.list_cursor = rows.saturating_sub(1),
            ShellAction::ListOpen => open = self.open_row(model).map(|target| Open { target, focus: true }),
            ShellAction::ListPeek => match list.get(self.list_cursor) {
                Some(Row::Section(i)) => {
                    self.collapsed.remove(&model.section(*i).id);
                }
                Some(Row::Conversation(_)) => {
                    open = self.open_row(model).map(|target| Open { target, focus: false });
                }
                _ => {}
            },
            ShellAction::ListLeft => match list.get(self.list_cursor) {
                Some(Row::Conversation(_)) => {
                    match list[..self.list_cursor].iter().rposition(|r| matches!(r, Row::Section(_))) {
                        Some(header) => self.list_cursor = header,
                        None => self.focus = Region::Rail,
                    }
                }
                Some(Row::Section(i)) => {
                    let id = model.section(*i).id.clone();
                    if !self.collapsed.insert(id) {
                        self.focus = Region::Rail;
                    }
                }
                _ => self.focus = Region::Rail,
            },
            ShellAction::ListSectionPrev | ShellAction::ListSectionNext => {
                let header = |r: &Row| matches!(r, Row::Section(_));
                let to = if action == ShellAction::ListSectionPrev {
                    list[..self.list_cursor].iter().rposition(header)
                } else {
                    list.iter().skip(self.list_cursor + 1).position(header).map(|i| i + self.list_cursor + 1)
                };
                if let Some(to) = to {
                    self.list_cursor = to;
                }
            }
            ShellAction::ToggleList => {
                self.list_hidden = !self.list_hidden;
                if self.list_hidden && self.focus == Region::List {
                    self.focus = Region::Work;
                }
            }
            ShellAction::Show(view) => self.select(RailItem::View(view), &items, model),
        }
        self.scroll(list_height);
        open
    }

    /// Put the list cursor on the row of the conversation `target` names, if the list shows it.
    pub fn reveal(&mut self, model: &Model, target: &Target, list_height: usize) {
        let rows = self.rows(model);
        let at = rows.iter().position(|r| match r {
            Row::Conversation(i) => {
                let c = model.conversation(*i);
                &c.workspace == target.workspace() && &c.id == target.conversation()
            }
            _ => false,
        });
        if let Some(at) = at {
            self.list_cursor = at;
            self.scroll(list_height);
        }
    }

    /// Scroll the list by `by` rows (the mouse wheel); the cursor stays on screen.
    pub fn scroll_by(&mut self, model: &Model, by: isize, list_height: usize) {
        let rows = self.rows(model);
        let max_top = rows.len().saturating_sub(list_height.max(1));
        self.list_top = self.list_top.saturating_add_signed(by).min(max_top);
        let bottom = self.list_top + list_height.max(1) - 1;
        let want = self.list_cursor.clamp(self.list_top, bottom.min(rows.len().saturating_sub(1)));
        if want != self.list_cursor {
            // Onto a row the cursor can stop on, toward the screen.
            let dir = if want > self.list_cursor { 1 } else { -1 };
            let mut at = want;
            while rows.get(at).is_some_and(|r| !r.is_selectable()) {
                at = at.saturating_add_signed(dir);
            }
            self.list_cursor = at.min(rows.len().saturating_sub(1));
        }
    }

    /// Show `item` (one of `items`, the rail) in the list panel and move the focus there.
    pub fn select(&mut self, item: RailItem, items: &[RailItem], model: &Model) {
        match item {
            RailItem::Workspace(ws) => {
                self.workspace = ws;
                self.view = View::Home;
            }
            RailItem::View(v) => self.view = v,
        }
        self.rail_cursor = items.iter().position(|i| *i == item).unwrap_or(self.rail_cursor);
        self.home_cursor(model);
        self.list_hidden = false;
        self.focus = Region::List;
    }

    /// The conversation under the cursor (to open), or fold or unfold its section.
    pub fn open_row(&mut self, model: &Model) -> Option<Target> {
        match self.rows(model).get(self.list_cursor) {
            Some(Row::Section(i)) => {
                let id = model.section(*i).id.clone();
                if !self.collapsed.remove(&id) {
                    self.collapsed.insert(id);
                }
                None
            }
            Some(Row::Conversation(i)) => {
                let c = model.conversation(*i);
                Some(Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() })
            }
            Some(Row::Spacer) | None => None,
        }
    }

    /// Put the list cursor on the first conversation (a view just shown starts there, not on a
    /// section header).
    pub fn home_cursor(&mut self, model: &Model) {
        let rows = self.rows(model);
        self.list_cursor = rows.iter().position(|r| matches!(r, Row::Conversation(_))).unwrap_or(0);
        self.list_top = 0;
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
        if model.is_loaded() && std::mem::take(&mut self.pending_home) {
            self.home_cursor(model);
        }
        self.list_cursor = self.list_cursor.min(self.rows(model).len().saturating_sub(1));
        self.scroll(list_height);
    }

    /// Where the focus goes from the rail without picking anything: the list, or the work area
    /// when the list is hidden.
    fn leave_rail(&self) -> Region {
        if self.list_hidden { Region::Work } else { Region::List }
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

/// The selectable row `by` rows from `at` (a blank row between sections is skipped), staying
/// inside the rows.
fn step(rows: &[Row], at: usize, by: isize) -> usize {
    let last = rows.len().saturating_sub(1);
    let mut to = at.saturating_add_signed(by).min(last);
    while rows.get(to).is_some_and(|r| !r.is_selectable()) && to != 0 && to != last {
        to = to.saturating_add_signed(by.signum()).min(last);
    }
    if rows.get(to).is_some_and(|r| r.is_selectable()) { to } else { at }
}

#[cfg(test)]
mod tests;
