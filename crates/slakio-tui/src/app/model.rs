//! The read model: what the backend said about the workspaces, people, sections and
//! conversations, with the queries the screen needs. Replaced as a whole by a boot answer; the
//! UI never changes it on its own.

use super::shell::View;
use super::todos::{Due, DueFilter, Group, Sort, StatusFilter, TodoView};
use slakio_core::backend::Snapshot;
use slakio_core::model::{
    Conversation, SavedItem, SavedState, Section, SectionId, Target, User, UserId, Workspace, WorkspaceId,
};
use std::collections::{HashMap, HashSet};

/// How a conversation is named in titles, tabs and the status line: `#backend`, `@Minsu`.
pub fn breadcrumb(c: &Conversation) -> String {
    let name = c.name.line();
    if c.is_dm() { format!("@{name}") } else { format!("#{name}") }
}

/// One row of the list panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// A section header (index into the sections).
    Section(usize),
    /// A conversation (index into the conversations).
    Conversation(usize),
    /// The blank row between two sections: never under the cursor.
    Spacer,
    /// A group of saved items: Home's Saved, To-dos' In progress and Completed.
    Group(Group),
    /// A saved item (index into the saved items): its text.
    Item(usize),
    /// A to-do's second line, who wrote it where and when: never under the cursor (the
    /// cursor on its item covers it).
    ItemMeta(usize),
    /// To-dos' first row: the filter and the order (`Enter`: the filter).
    Filter,
}

impl Row {
    /// The cursor can stop here.
    pub fn is_selectable(self) -> bool {
        !matches!(self, Row::Spacer | Row::ItemMeta(_))
    }

    /// A header that folds: a section or a group.
    pub fn is_header(self) -> bool {
        matches!(self, Row::Section(_) | Row::Group(_))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    snapshot: Snapshot,
    /// Index into the people, by id.
    users: HashMap<UserId, usize>,
    /// Set once the backend answered.
    loaded: bool,
}

impl Model {
    pub fn new(snapshot: Snapshot) -> Self {
        let users = snapshot.users.iter().enumerate().map(|(i, u)| (u.id.clone(), i)).collect();
        Self { snapshot, users, loaded: true }
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn workspaces(&self) -> &[Workspace] {
        &self.snapshot.workspaces
    }

    pub fn section(&self, i: usize) -> &Section {
        &self.snapshot.sections[i]
    }

    /// Every conversation of every workspace.
    pub fn conversations(&self) -> &[Conversation] {
        &self.snapshot.conversations
    }

    pub fn conversation(&self, i: usize) -> &Conversation {
        &self.snapshot.conversations[i]
    }

    /// The conversation `target` names (a thread's conversation for a thread).
    pub fn target(&self, target: &Target) -> Option<&Conversation> {
        let (workspace, conversation) = (target.workspace(), target.conversation());
        self.snapshot.conversations.iter().find(|c| &c.workspace == workspace && &c.id == conversation)
    }

    pub fn user(&self, id: &UserId) -> Option<&User> {
        self.users.get(id).map(|&i| &self.snapshot.users[i])
    }

    pub fn workspace(&self, id: &WorkspaceId) -> Option<&Workspace> {
        self.snapshot.workspaces.iter().find(|w| &w.id == id)
    }

    /// The user's own account in workspace `id`.
    pub fn me(&self, id: &WorkspaceId) -> Option<&UserId> {
        self.workspace(id).map(|w| &w.me)
    }

    /// When the backend took its snapshot (its clock for reminders).
    pub fn taken(&self) -> slakio_core::model::Ts {
        self.snapshot.taken
    }

    pub fn saved(&self, i: usize) -> &SavedItem {
        &self.items()[i]
    }

    /// Every saved item; none while the backend cannot tell ([`Model::saved_known`]).
    fn items(&self) -> &[SavedItem] {
        self.snapshot.saved.as_deref().unwrap_or_default()
    }

    /// The backend told which items are saved (else To-dos is unavailable, not empty).
    pub fn saved_known(&self) -> bool {
        self.snapshot.saved.is_some()
    }

    /// Every saved item after a change the backend made, and its clock then.
    pub fn set_saved(&mut self, items: Vec<SavedItem>, taken: slakio_core::model::Ts) {
        self.snapshot.saved = Some(items);
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
    pub fn todo_count(&self, ws: usize) -> (u32, bool) {
        let Some(w) = self.snapshot.workspaces.get(ws) else { return (0, false) };
        let open = self.items().iter().filter(|s| s.workspace == w.id && s.state == SavedState::InProgress);
        open.fold((0, false), |(n, late), s| (n + 1, late || self.due(s) == Due::Overdue))
    }

    /// The items of group `g` in workspace `ws` that `opts` lets through, in its order.
    pub fn group_items(&self, ws: usize, g: Group, opts: &TodoView) -> Vec<usize> {
        let Some(w) = self.snapshot.workspaces.get(ws) else { return vec![] };
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
                s.workspace == w.id && s.state == state && (!todo || due_ok(s))
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
    fn todo_rows(&self, ws: usize, groups: &HashSet<Group>, opts: &TodoView) -> Vec<Row> {
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

    /// The rows of `view` in workspace `ws`, sections in `collapsed` and groups in `groups`
    /// folded, To-dos as `opts` filters and orders them.
    pub fn rows(
        &self,
        ws: usize,
        view: View,
        collapsed: &HashSet<SectionId>,
        groups: &HashSet<Group>,
        opts: &TodoView,
    ) -> Vec<Row> {
        let Some(w) = self.snapshot.workspaces.get(ws) else { return vec![] };
        let convs = self.snapshot.conversations.iter().enumerate().filter(|(_, c)| c.workspace == w.id);
        match view {
            View::Home => {
                let mut rows = vec![];
                for (si, s) in self.snapshot.sections.iter().enumerate().filter(|(_, s)| s.workspace == w.id) {
                    if !rows.is_empty() {
                        rows.push(Row::Spacer);
                    }
                    rows.push(Row::Section(si));
                    if !collapsed.contains(&s.id) {
                        rows.extend(
                            convs.clone().filter(|(_, c)| c.section == s.id).map(|(i, _)| Row::Conversation(i)),
                        );
                    }
                }
                // Saved messages, after the sections (Slack's Home "Saved").
                let saved = self.group_items(ws, Group::Saved, opts);
                if !saved.is_empty() {
                    rows.push(Row::Spacer);
                    rows.push(Row::Group(Group::Saved));
                    if !groups.contains(&Group::Saved) {
                        rows.extend(saved.into_iter().map(Row::Item));
                    }
                }
                rows
            }
            View::Todos => self.todo_rows(ws, groups, opts),
            View::Dms => convs.filter(|(_, c)| c.is_dm()).map(|(i, _)| Row::Conversation(i)).collect(),
            // Not built yet: the list panel says so.
            View::Activity | View::Files => vec![],
        }
    }

    /// Of the conversations of section `i`: how many have unread messages (not muted ones), and
    /// the mentions they hold (what a folded header shows).
    pub fn section_counts(&self, i: usize) -> (u32, u32) {
        let s = &self.snapshot.sections[i];
        let convs = self.snapshot.conversations.iter().filter(|c| c.workspace == s.workspace && c.section == s.id);
        convs.fold((0, 0), |(u, m), c| (u + u32::from(c.unread > 0 && !c.muted), m + c.mentions))
    }

    /// Unread mentions in workspace `ws`.
    pub fn workspace_mentions(&self, ws: usize) -> u32 {
        let Some(w) = self.snapshot.workspaces.get(ws) else { return 0 };
        self.snapshot.conversations.iter().filter(|c| c.workspace == w.id).map(|c| c.mentions).sum()
    }

    /// Workspace `ws` has an unread conversation that is not muted.
    pub fn workspace_unread(&self, ws: usize) -> bool {
        self.snapshot.workspaces.get(ws).is_some_and(|w| {
            self.snapshot.conversations.iter().any(|c| c.workspace == w.id && c.unread > 0 && !c.muted)
        })
    }

    /// Unread DM messages of workspace `ws`.
    pub fn dm_unread(&self, ws: usize) -> u32 {
        let Some(w) = self.snapshot.workspaces.get(ws) else { return 0 };
        self.snapshot.conversations.iter().filter(|c| c.workspace == w.id && c.is_dm()).map(|c| c.unread).sum()
    }

    /// Unread mentions across every workspace (what Activity collects).
    pub fn mentions(&self) -> u32 {
        self.snapshot.conversations.iter().map(|c| c.mentions).sum()
    }
}

#[cfg(test)]
mod tests;
