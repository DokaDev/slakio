//! The saved items as the list panel shows them: To-dos' groups, filtered and ordered, Home's
//! Saved, and the reminders' state by the backend's clock.

use super::{Model, Row};
use crate::app::todos::{Due, DueFilter, Group, Sort, StatusFilter, TodoView};
use slakio_core::model::{SavedItem, SavedState, WorkspaceId};
use std::collections::HashSet;

impl Model {
    pub fn saved(&self, i: usize) -> &SavedItem {
        &self.items()[i]
    }

    /// Every saved item; none while the backend cannot tell ([`Model::saved_known`]).
    pub(super) fn items(&self) -> &[SavedItem] {
        self.snapshot.saved.as_deref().unwrap_or_default()
    }

    /// The backend told which items of workspace `ws` are saved (else To-dos is unavailable,
    /// not empty).
    pub fn saved_known(&self, ws: &WorkspaceId) -> bool {
        !self.saved_unknown.contains(ws)
    }

    /// Workspace `ws`'s saved items after a change its backend made, and its clock then: the
    /// other workspaces' items stay, and an item of another workspace is left out.
    pub fn set_saved_in(&mut self, ws: &WorkspaceId, items: Vec<SavedItem>, taken: slakio_core::model::Ts) {
        let mut all = self.snapshot.saved.take().unwrap_or_default();
        all.retain(|i| &i.workspace != ws);
        all.extend(items.into_iter().filter(|i| &i.workspace == ws));
        self.saved_unknown.remove(ws);
        self.snapshot.saved = Some(all);
        self.snapshot.taken = taken;
    }

    /// Whether item `s`'s reminder is overdue (only a to-do in progress is), upcoming or none,
    /// by the backend's clock.
    pub fn due(&self, s: &SavedItem) -> Due {
        match s.due {
            None => Due::None,
            Some(d) if d < self.snapshot.taken && s.state == SavedState::InProgress => Due::Overdue,
            Some(d) if d < self.snapshot.taken => Due::Past,
            Some(_) => Due::Upcoming,
        }
    }

    /// The to-dos of workspace `ws` in progress, and whether one is overdue (the view's count).
    pub fn todo_count(&self, ws: &WorkspaceId) -> (u32, bool) {
        let open = self.items().iter().filter(|s| &s.workspace == ws && s.state == SavedState::InProgress);
        open.fold((0, false), |(n, late), s| (n + 1, late || self.due(s) == Due::Overdue))
    }

    /// The items of group `g` in workspace `ws` that `opts` lets through, in its order.
    pub fn group_items(&self, ws: &WorkspaceId, g: Group, opts: &TodoView) -> Vec<usize> {
        let state = match g {
            Group::Saved => SavedState::Saved,
            Group::InProgress => SavedState::InProgress,
            Group::Completed => SavedState::Completed,
        };
        let todo = g != Group::Saved;
        let due_ok = |s: &SavedItem| match (opts.due, self.due(s)) {
            (DueFilter::Any, _) | (DueFilter::Overdue, Due::Overdue) | (DueFilter::Upcoming, Due::Upcoming) => true,
            (DueFilter::NoReminder, d) => d == Due::None,
            _ => false,
        };
        let mut out: Vec<usize> = (0..self.items().len())
            .filter(|&i| {
                let s = &self.items()[i];
                &s.workspace == ws && s.state == state && (!todo || due_ok(s))
            })
            .collect();
        let key = |i: &usize| self.items()[*i].created;
        match opts.sort {
            _ if !todo => out.sort_by_key(|i| std::cmp::Reverse(key(i))),
            Sort::Due => out.sort_by_key(|i| (self.items()[*i].due.is_none(), self.items()[*i].due, key(i))),
            Sort::Oldest => out.sort_by_key(key),
            Sort::Newest => out.sort_by_key(|i| std::cmp::Reverse(key(i))),
        }
        out
    }

    /// The rows of To-dos: the filter row, then In progress and Completed (as `opts` lets
    /// them through, and not when a filter leaves them empty), each a header and two rows per
    /// item unless folded (`groups`).
    pub(super) fn todo_rows(&self, ws: &WorkspaceId, groups: &HashSet<Group>, opts: &TodoView) -> Vec<Row> {
        let mut rows = vec![Row::Filter];
        let shown = [(Group::InProgress, StatusFilter::Completed), (Group::Completed, StatusFilter::InProgress)];
        for (g, hidden_by) in shown {
            let items = self.group_items(ws, g, opts);
            // A filter hides the groups it leaves empty.
            if opts.status == hidden_by || (items.is_empty() && opts.is_filtered()) {
                continue;
            }
            rows.push(Row::Spacer);
            rows.push(Row::Group(g));
            if !groups.contains(&g) {
                rows.extend(items.into_iter().flat_map(|i| [Row::Item(i), Row::ItemMeta(i)]));
            }
        }
        rows
    }
}
