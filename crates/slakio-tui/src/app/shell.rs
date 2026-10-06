//! The shell: the regions the focus moves between (the view switcher, the list panel, the work
//! area), the view switcher's cursor, the workspace and the view the list panel shows, the list's
//! cursor and scroll, and folded sections. It owns its update ([`Shell::update`]), which hands
//! back a conversation to open (the work area opens it); the rows come from the read model.
//!
//! ```text
//! ╭ ▌A company ▾ ───╮
//! │ 󰋜  Home  󰍡  ●2  │   the view switcher            work area
//! ├─────────────────┤
//! │ ▾ Favorites     │   the list
//! │   # backend   3 │
//! ```

use super::model::{Model, Row};
use crate::action::ShellAction;
use slakio_core::i18n::Label;
use slakio_core::model::{SectionId, Target};

/// A conversation to open from the list: `focus` moves the keyboard to it (`Enter`), else the
/// list keeps it (`l`, a peek).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    pub target: Target,
    pub focus: bool,
}
use std::collections::HashSet;

/// The regions of the main screen, in reading order: the view switcher (the list panel's first
/// row), the list, the work area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    ViewSwitcher,
    List,
    Work,
}

/// What the list panel shows, picked on the view switcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum View {
    Home,
    Dms,
    Activity,
    Files,
    Later,
}

impl View {
    /// In the view switcher's order.
    pub const ALL: &'static [View] = &[View::Home, View::Dms, View::Activity, View::Files, View::Later];

    /// The view's name (the view switcher, the breadcrumb).
    pub fn label(self) -> Label {
        match self {
            View::Home => Label::NavHome,
            View::Dms => Label::NavDms,
            View::Activity => Label::NavActivity,
            View::Files => Label::NavFiles,
            View::Later => Label::NavLater,
        }
    }

    /// Its Nerd Font glyph (`icons`), else its letter.
    pub fn glyph(self, icons: bool) -> &'static str {
        match (self, icons) {
            (View::Home, false) => "H",
            (View::Dms, false) => "D",
            (View::Activity, false) => "A",
            (View::Files, false) => "F",
            (View::Later, false) => "L",
            (View::Home, true) => "\u{F02DC}",
            (View::Dms, true) => "\u{F0361}",
            (View::Activity, true) => "\u{F009A}",
            (View::Files, true) => "\u{F0219}",
            (View::Later, true) => "\u{F00C0}",
        }
    }

    /// Built in a later version: the list panel shows a placeholder, never invented data that
    /// could pass for the real thing.
    pub fn is_placeholder(self) -> bool {
        matches!(self, View::Activity | View::Files | View::Later)
    }
}

#[derive(Clone, Debug)]
pub struct Shell {
    /// The view switcher's cursor: index into [`View::ALL`].
    pub nav_cursor: usize,
    /// The workspace the list panel shows.
    pub workspace: usize,
    pub view: View,
    /// Index into the list's rows.
    pub list_cursor: usize,
    /// The first row on screen.
    pub list_top: usize,
    pub collapsed: HashSet<SectionId>,
    pub list_hidden: bool,
    /// The list cursor goes to the first conversation once the rows are known.
    pending_home: bool,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            // On the view shown (Home).
            nav_cursor: 0,
            workspace: 0,
            view: View::Home,
            list_cursor: 0,
            list_top: 0,
            collapsed: HashSet::new(),
            list_hidden: false,
            pending_home: true,
        }
    }
}

impl Shell {
    pub fn rows(&self, model: &Model) -> Vec<Row> {
        model.rows(self.workspace, self.view, &self.collapsed)
    }

    /// Apply `action`. `list_height` is the number of rows the list panel shows (for scrolling
    /// and paging); `here` is the region with the focus, which the action may move (the app
    /// moves the focus there). The conversation to open, when the action opens one.
    pub fn update(
        &mut self,
        action: ShellAction,
        model: &Model,
        list_height: usize,
        here: &mut Region,
    ) -> Option<Open> {
        let mut open = None;
        let list = self.rows(model);
        let rows = list.len();
        let page = list_height.max(1) as isize;
        match action {
            ShellAction::FocusLeft => *here = self.neighbour(*here, -1),
            ShellAction::FocusRight => *here = self.neighbour(*here, 1),
            // Up, down, next and previous need the work area: the app moves those.
            ShellAction::FocusUp | ShellAction::FocusDown | ShellAction::FocusNext | ShellAction::FocusPrev => {}
            ShellAction::FocusNav
            | ShellAction::ViewNext
            | ShellAction::ViewPrev
            | ShellAction::NavNext
            | ShellAction::NavPrev
            | ShellAction::NavFirst
            | ShellAction::NavLast
            | ShellAction::NavSelect
            | ShellAction::NavLeave
            | ShellAction::SwitcherNext
            | ShellAction::SwitcherPrev
            | ShellAction::SwitcherChoose
            | ShellAction::SwitcherClose => self.nav(action, model, here),
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
                        None => *here = Region::ViewSwitcher,
                    }
                }
                Some(Row::Section(i)) => {
                    let id = model.section(*i).id.clone();
                    if !self.collapsed.insert(id) {
                        *here = Region::ViewSwitcher;
                    }
                }
                _ => *here = Region::ViewSwitcher,
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
                if self.list_hidden && matches!(*here, Region::List | Region::ViewSwitcher) {
                    *here = Region::Work;
                }
            }
            ShellAction::Show(view) => {
                self.select(view, model);
                *here = Region::List;
            }
        }
        self.scroll(list_height);
        open
    }

    /// The view switcher's keys and `[` / `]` (the workspace switcher's are the app's).
    fn nav(&mut self, action: ShellAction, model: &Model, here: &mut Region) {
        let last_item = View::ALL.len() - 1;
        match action {
            ShellAction::FocusNav if *here == Region::ViewSwitcher => *here = self.leave_nav(),
            // The view switcher is the list panel's: it shows again.
            ShellAction::FocusNav => {
                self.list_hidden = false;
                self.nav_cursor = self.view_index();
                *here = Region::ViewSwitcher;
            }
            ShellAction::ViewNext | ShellAction::ViewPrev => {
                let by = if action == ShellAction::ViewNext { 1 } else { last_item };
                self.select(View::ALL[(self.view_index() + by) % View::ALL.len()], model);
            }
            ShellAction::NavNext => self.nav_cursor = (self.nav_cursor + 1).min(last_item),
            ShellAction::NavPrev => self.nav_cursor = self.nav_cursor.saturating_sub(1),
            ShellAction::NavFirst => self.nav_cursor = 0,
            ShellAction::NavLast => self.nav_cursor = last_item,
            ShellAction::NavSelect => {
                self.select(View::ALL[self.nav_cursor.min(last_item)], model);
                *here = Region::List;
            }
            ShellAction::NavLeave => *here = self.leave_nav(),
            // The app keeps the switcher.
            ShellAction::SwitcherNext
            | ShellAction::SwitcherPrev
            | ShellAction::SwitcherChoose
            | ShellAction::SwitcherClose => {}
            _ => {}
        }
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

    /// Show view `v` in the list panel (the focus goes there: the app moves it).
    pub fn select(&mut self, v: View, model: &Model) {
        self.view = v;
        self.nav_cursor = self.view_index();
        self.home_cursor(model);
        self.list_hidden = false;
    }

    /// Show workspace `ws` (index into the model's workspaces), from its Home.
    pub fn select_workspace(&mut self, ws: usize, model: &Model) {
        self.workspace = ws.min(model.workspaces().len().saturating_sub(1));
        self.view = View::Home;
        self.nav_cursor = 0;
        self.home_cursor(model);
        self.list_hidden = false;
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

    /// The conversation under the cursor, if the cursor is on one.
    pub fn target_at_cursor(&self, model: &Model) -> Option<Target> {
        match self.rows(model).get(self.list_cursor) {
            Some(Row::Conversation(i)) => {
                let c = model.conversation(*i);
                Some(Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() })
            }
            _ => None,
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
        self.nav_cursor = self.nav_cursor.min(View::ALL.len() - 1);
        if model.is_loaded() && std::mem::take(&mut self.pending_home) {
            self.home_cursor(model);
        }
        self.list_cursor = self.list_cursor.min(self.rows(model).len().saturating_sub(1));
        self.scroll(list_height);
    }

    /// The view shown: index into [`View::ALL`].
    fn view_index(&self) -> usize {
        View::ALL.iter().position(|v| *v == self.view).unwrap_or(0)
    }

    /// Where the focus goes from the view switcher without picking anything: the list, or the
    /// work area when the list is hidden.
    fn leave_nav(&self) -> Region {
        if self.list_hidden { Region::Work } else { Region::List }
    }

    /// The region `step` places before (-1) or after (1) `here`, skipping a hidden list panel
    /// (and its view switcher).
    fn neighbour(&self, here: Region, step: i8) -> Region {
        let order: Vec<Region> = [Region::ViewSwitcher, Region::List, Region::Work]
            .into_iter()
            .filter(|r| *r == Region::Work || !self.list_hidden)
            .collect();
        let at = order.iter().position(|r| *r == here).unwrap_or(0);
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
