//! To-dos and saved messages as the app routes them: how To-dos is filtered (by state and by
//! reminder) and ordered, the changes to an item the keys ask for — complete or reopen (`x`),
//! move to Saved or back to To-dos (`m`), clear (`d`, asked first) — and the backend's answer.
//!
//! A change is a request ([`Command::Save`]); nothing changes on screen until the backend
//! answers with every item after it (sent is not succeeded), and an answer to an older
//! request is dropped. `Enter` on an item opens its message (its thread when it has one) with the
//! message selected; `f` and `s` (`:filter`, `:sort`) open the palette on their values.
//!
//! ```text
//! │ All ▾ · Due date ▾           │   the filter row (a click on either part opens its values)
//! │                              │
//! │ ▾ In progress              6 │
//! │ MK Check the dep…    Overdue │   an item: its author, its text, its reminder,
//! │    #backend · 01-05          │   then where and when (who too, when it fits)
//! ```

use super::dialog::{Dialog, Question};
use super::model::Row;
use super::palette::{Item, Row as PaletteRow};
use super::shell::View;
use super::{App, Focus};
use crate::action::ShellAction;
use crate::text::{clip, width};
use slakio_core::backend::{Command, Generation, SavedChange};
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::{SavedId, SavedItem, SavedState, Ts, WorkspaceId};
use std::time::Instant;

/// Which to-dos To-dos shows by their state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusFilter {
    #[default]
    All,
    InProgress,
    Completed,
}

/// Which to-dos To-dos shows by their reminder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DueFilter {
    #[default]
    Any,
    Overdue,
    Upcoming,
    NoReminder,
}

/// The order of To-dos' items.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    /// The soonest reminder first, those without one last.
    #[default]
    Due,
    /// Saved earliest first.
    Oldest,
    Newest,
}

/// An item's reminder, by the backend's clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    /// A to-do in progress whose reminder is past.
    Overdue,
    Upcoming,
    /// Past, on a completed to-do: nothing is late any more.
    Past,
    None,
}

/// A group of saved items in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    /// Home's saved messages.
    Saved,
    InProgress,
    Completed,
}

/// How To-dos is filtered and ordered, and the request the next answer must answer.
#[derive(Clone, Debug, Default)]
pub struct TodoView {
    pub status: StatusFilter,
    pub due: DueFilter,
    pub sort: Sort,
    /// The last change asked for (older answers are dropped) and what it was; its notice counts
    /// from the answer.
    asked: Option<(Generation, SavedChange)>,
    /// The item `d` asked about (and its workspace), while the question is open.
    clearing: Option<(SavedId, WorkspaceId)>,
}

/// The values `:filter` takes: `all` shows every to-do again.
pub const FILTER_VALUES: &[&str] = &["all", "in-progress", "completed", "overdue", "upcoming", "no-reminder"];

/// The values `:sort` takes.
pub const SORT_VALUES: &[&str] = &["due", "oldest", "newest"];

fn filter_label(value: &str) -> Label {
    match value {
        "in-progress" => Label::FilterInProgress,
        "completed" => Label::FilterCompleted,
        "overdue" => Label::FilterOverdue,
        "upcoming" => Label::FilterUpcoming,
        "no-reminder" => Label::FilterNoReminder,
        _ => Label::FilterAll,
    }
}

fn sort_label(s: Sort) -> Label {
    match s {
        Sort::Due => Label::SortDue,
        Sort::Oldest => Label::SortOldest,
        Sort::Newest => Label::SortNewest,
    }
}

impl TodoView {
    /// Something filters To-dos.
    pub fn is_filtered(&self) -> bool {
        self.status != StatusFilter::All || self.due != DueFilter::Any
    }

    /// The `:filter` values in effect (`all` when nothing filters).
    fn filters(&self) -> Vec<&'static str> {
        let status = match self.status {
            StatusFilter::All => None,
            StatusFilter::InProgress => Some("in-progress"),
            StatusFilter::Completed => Some("completed"),
        };
        let due = match self.due {
            DueFilter::Any => None,
            DueFilter::Overdue => Some("overdue"),
            DueFilter::Upcoming => Some("upcoming"),
            DueFilter::NoReminder => Some("no-reminder"),
        };
        let on: Vec<&str> = [status, due].into_iter().flatten().collect();
        if on.is_empty() { vec!["all"] } else { on }
    }
}

/// The filter row as laid out: the filter's text, the order's, and the column the order starts
/// at (from the row's left edge; a click from there on opens the order's values).
pub struct FilterRow {
    pub filter: String,
    pub sort: String,
    pub sort_x: u16,
}

impl App {
    /// The filter row of To-dos: the filter (two of them as their count, `2 filters`), then the
    /// order, which always stays: short of room the order takes its short name, then the filter
    /// is cut, then the order.
    pub fn filter_row(&self) -> FilterRow {
        let label = |l: Label| self.i18n.label(l).to_string();
        let on = self.shell.todo.filters();
        let filter = match on.as_slice() {
            [one] => label(filter_label(one)),
            _ => self.i18n.msg(&Msg::FilterCount { count: on.len() as u64 }).to_string(),
        };
        // The row's cells inside its gutter and right padding, less ` ▾ · ` and ` ▾`.
        let room = self.list_parts().map_or(28, |p| usize::from(p.rows.width)).saturating_sub(2 + 5 + 2);
        let mut sort = label(sort_label(self.shell.todo.sort));
        if width(&filter) + width(&sort) > room {
            sort = label(match self.shell.todo.sort {
                Sort::Due => Label::SortDueShort,
                Sort::Oldest => Label::SortOldestShort,
                Sort::Newest => Label::SortNewestShort,
            });
        }
        let sort = clip(&sort, room.saturating_sub(width(&filter).min(4)));
        let filter = clip(&filter, room.saturating_sub(width(&sort)));
        let (filter, sort) = (format!("{filter} ▾"), format!("{sort} ▾"));
        let sort_x = 1 + width(&filter) as u16 + 3;
        FilterRow { filter, sort, sort_x }
    }

    /// The saved item under the list's cursor.
    fn item(&self) -> Option<&SavedItem> {
        self.shell.item_at_cursor(&self.model).map(|i| self.model.saved(i))
    }

    /// The keys of To-dos and Saved (see the module's text). A backend that changes nothing
    /// refuses every change, saying so.
    pub(super) fn todo(&mut self, a: ShellAction, now: Instant) {
        let state = self.item().map(|s| ((s.id.clone(), s.workspace.clone()), s.state));
        let change = match (a, state) {
            (ShellAction::TodoFilter, _) => return self.palette_with("filter "),
            (ShellAction::TodoSort, _) => return self.palette_with("sort "),
            (_, None) => return self.info(Msg::Label(Label::StatusNotSaved), now),
            _ if self.read_only() => return self.info(Msg::Label(Label::FailReadOnly), now),
            (ShellAction::TodoToggle, Some((_, SavedState::Saved))) => {
                return self.info(Msg::Label(Label::StatusNotTodo), now);
            }
            (ShellAction::TodoToggle, Some((id, SavedState::InProgress))) => (id, SavedChange::Complete),
            (ShellAction::TodoToggle, Some((id, _))) => (id, SavedChange::Reopen),
            (ShellAction::TodoMove, Some((id, SavedState::Saved))) => (id, SavedChange::MoveToTodos),
            (ShellAction::TodoMove, Some((id, _))) => (id, SavedChange::MoveToSaved),
            (_, Some((id, _))) => {
                self.help = None;
                self.shell.todo.clearing = Some(id);
                self.dialog = Some(Dialog::new(Question::Clear));
                return;
            }
        };
        self.ask_saved(change.0, change.1);
    }

    fn ask_saved(&mut self, (item, workspace): (SavedId, WorkspaceId), change: SavedChange) {
        let g = self.work.requests.ask(Some(workspace), Command::Save { item, change });
        self.shell.todo.asked = Some((g, change));
    }

    /// The answer to the clear question: `yes` clears the item asked about.
    pub(super) fn answer_clear(&mut self, yes: bool) {
        if let Some(item) = self.shell.todo.clearing.take().filter(|_| yes) {
            self.ask_saved(item, SavedChange::Clear);
        }
    }

    /// The backend's saved items after the change asked last, by its clock `taken`, arrived at
    /// `now`; an older answer is dropped.
    pub(super) fn on_saved(&mut self, generation: Generation, items: Vec<SavedItem>, taken: Ts, now: Instant) -> bool {
        let Some((_, change)) = self.shell.todo.asked.filter(|(g, _)| *g == generation) else { return false };
        self.shell.todo.asked = None;
        self.model.set_saved(items, taken);
        self.shell.clamp(&self.model, self.list_height());
        let done = match change {
            SavedChange::Complete => Label::StatusTodoCompleted,
            SavedChange::Reopen => Label::StatusTodoReopened,
            SavedChange::MoveToSaved => Label::StatusTodoSaved,
            SavedChange::MoveToTodos => Label::StatusTodoBack,
            SavedChange::Clear => Label::StatusTodoCleared,
        };
        self.info(Msg::Label(done), now);
        true
    }

    /// The change asked last as request `id` failed: nothing changed, and nothing waits for it.
    pub(super) fn on_save_failed(&mut self, id: Generation) -> bool {
        self.shell.todo.asked.take_if(|(g, _)| *g == id).is_some()
    }

    /// `Enter` (`focus`) or `l` on an item: its message in the work area, selected — in its
    /// thread when it is a thread's own message.
    pub(super) fn open_item(&mut self, focus: bool) {
        let (Some(s), Some(target)) = (self.item(), self.shell.target_at_cursor(&self.model)) else { return };
        let ts = s.ts;
        self.open(target, focus);
        if let Some(p) = self.work.focused_mut() {
            p.selected = Some(ts);
        }
    }

    /// The cursor is on To-dos' filter row.
    pub(super) fn on_filter_row(&self) -> bool {
        self.shell.rows(&self.model).get(self.shell.list_cursor) == Some(&Row::Filter)
    }

    /// The palette open on `text` (a command waiting for its value).
    fn palette_with(&mut self, text: &str) {
        self.help = None;
        self.cmdline.open();
        self.cmdline.set(text);
    }

    /// `:filter <value>`: To-dos shows only those to-dos (a state and a reminder filter
    /// together; `all` clears both), from its first row.
    pub fn set_todo_filter(&mut self, value: &str, now: Instant) -> Result<(), Msg> {
        let value = value.trim().to_ascii_lowercase();
        let t = &mut self.shell.todo;
        match value.as_str() {
            "all" => (t.status, t.due) = (StatusFilter::All, DueFilter::Any),
            "in-progress" => t.status = StatusFilter::InProgress,
            "completed" => t.status = StatusFilter::Completed,
            "overdue" => t.due = DueFilter::Overdue,
            "upcoming" => t.due = DueFilter::Upcoming,
            "no-reminder" => t.due = DueFilter::NoReminder,
            _ => return Err(Msg::FilterUnknown { name: value, names: FILTER_VALUES.join(", ") }),
        }
        self.show_todos();
        let label = |v: &str| self.i18n.label(filter_label(v)).to_string();
        let name = self.shell.todo.filters().iter().map(|v| label(v)).collect::<Vec<_>>().join(", ");
        self.info(Msg::FilterChanged { name }, now);
        Ok(())
    }

    /// `:sort <value>`: To-dos in that order.
    pub fn set_todo_sort(&mut self, value: &str, now: Instant) -> Result<(), Msg> {
        let value = value.trim().to_ascii_lowercase();
        self.shell.todo.sort = match value.as_str() {
            "due" => Sort::Due,
            "oldest" => Sort::Oldest,
            "newest" => Sort::Newest,
            _ => return Err(Msg::SortUnknown { name: value, names: SORT_VALUES.join(", ") }),
        };
        self.show_todos();
        let name = self.i18n.label(sort_label(self.shell.todo.sort)).to_string();
        self.info(Msg::SortChanged { name }, now);
        Ok(())
    }

    /// To-dos in the list panel, the cursor on its first item.
    fn show_todos(&mut self) {
        self.shell.select(View::Todos, &self.model);
        if self.focus() == Focus::ViewSwitcher || self.focus() == Focus::List {
            self.set_focus(Focus::List);
        }
        let h = self.list_height();
        self.shell.scroll(h);
    }

    /// The palette's rows for the values of `:filter` and `:sort` (`current` on those in effect).
    pub(super) fn todo_palette_row(&self, item: Item) -> Option<PaletteRow> {
        let current = || self.i18n.label(Label::PaletteCurrent).to_string();
        let (name, label, on) = match item {
            Item::TodoFilter(i) => {
                let v = FILTER_VALUES[i];
                (v, filter_label(v), self.shell.todo.filters().contains(&v))
            }
            Item::TodoSort(i) => {
                let s = [Sort::Due, Sort::Oldest, Sort::Newest][i];
                (SORT_VALUES[i], sort_label(s), s == self.shell.todo.sort)
            }
            _ => return None,
        };
        let keys = if on { current() } else { String::new() };
        Some(PaletteRow { name: name.to_string(), label: self.i18n.label(label).to_string(), keys })
    }

    /// Run the palette's `:filter` or `:sort` value `item`.
    pub(super) fn run_todo_item(&mut self, item: Item, now: Instant) {
        let done = match item {
            Item::TodoFilter(i) => self.set_todo_filter(FILTER_VALUES[i], now),
            Item::TodoSort(i) => self.set_todo_sort(SORT_VALUES[i], now),
            _ => Ok(()),
        };
        if let Err(msg) = done {
            self.warn(msg, now);
        }
    }
}
