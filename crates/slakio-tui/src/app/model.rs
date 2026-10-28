//! The read model: what the backend said about the workspaces, people, sections and
//! conversations, with the queries the screen needs. A boot answer of every workspace replaces
//! it whole; one workspace's replaces that workspace's part only, so the others stay as they
//! were. The UI never changes it on its own. Everything in it is found by id: people by the
//! workspace that lists them and their id, a list row's cursor by the row's id ([`RowKey`]).

use super::shell::View;
use super::todos::{Due, DueFilter, Group, Sort, StatusFilter, TodoView};
use slakio_core::backend::Snapshot;
use slakio_core::model::{
    Conversation, ConversationId, SavedId, SavedItem, SavedState, Section, SectionId, Target, User, UserId, Workspace,
    WorkspaceColor, WorkspaceId,
};
use slakio_core::sanitize::Remote;
use std::collections::{HashMap, HashSet};

/// How a conversation is named in titles, tabs and the status line: `#backend`, `@Minsu`.
pub fn breadcrumb(c: &Conversation) -> String {
    let name = c.name.line();
    if c.is_dm() { format!("@{name}") } else { format!("#{name}") }
}

/// One row of the list panel. Its index is into the model it came from and holds for that model
/// only: across a change of the model the cursor goes by the row's id ([`Model::key`]).
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

/// What a list row is, by id: the cursor is kept on it when the rows change under it (a boot
/// answer, another workspace's, adds or removes rows), never on whatever row now has its index.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RowKey {
    Section(SectionId),
    Conversation(ConversationId),
    Group(Group),
    Item(SavedId),
    Filter,
}

/// A workspace the user is signed in to, before (or whether or not) its boot answers: the
/// switcher lists it, and its list says it is loading or could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedIn {
    pub id: WorkspaceId,
    /// Its name as the sign-in kept it (remote text).
    pub name: String,
    /// Nothing can be changed in it.
    pub read_only: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    snapshot: Snapshot,
    /// Index into the people, by the workspace that lists them and their id: a Slack Connect
    /// peer listed by two workspaces is two entries, each with its workspace's view of them.
    users: HashMap<(WorkspaceId, UserId), usize>,
    /// The workspaces whose backend answered.
    fresh: HashSet<WorkspaceId>,
    /// The workspaces shown from the local cache (an earlier run's) until the backend answers.
    cached: HashSet<WorkspaceId>,
    /// The workspaces whose saved items the backend cannot tell (unknown is not empty).
    saved_unknown: HashSet<WorkspaceId>,
    /// The workspaces whose unread counts Slack could not tell, with none kept from before:
    /// their views show `?`, not a look of all read.
    pub counts_unknown: HashSet<WorkspaceId>,
}

impl Model {
    /// Every workspace's data at once (a backend that boots them together, the demo).
    pub fn new(snapshot: Snapshot) -> Self {
        let ids: HashSet<WorkspaceId> = snapshot.workspaces.iter().map(|w| w.id.clone()).collect();
        let saved_unknown = if snapshot.saved.is_none() { ids.clone() } else { HashSet::new() };
        let mut m = Self {
            snapshot,
            users: HashMap::new(),
            fresh: ids,
            cached: HashSet::new(),
            saved_unknown,
            counts_unknown: HashSet::new(),
        };
        m.index_users();
        m
    }

    /// The local cache's snapshot from an earlier run, until the backend answers.
    pub fn cached(snapshot: Snapshot) -> Self {
        let mut m = Self::new(snapshot);
        m.cached = std::mem::take(&mut m.fresh);
        m
    }

    fn index_users(&mut self) {
        self.users =
            self.snapshot.users.iter().enumerate().map(|(i, u)| ((u.workspace.clone(), u.id.clone()), i)).collect();
    }

    /// The workspaces signed in to, in this order: one not listed yet is listed by its name
    /// until its boot answers (its color is its place); one no longer signed in goes, with
    /// everything of it.
    pub fn sign_ins(&mut self, signed: &[SignedIn]) {
        let keep: HashSet<&WorkspaceId> = signed.iter().map(|s| &s.id).collect();
        let s = &mut self.snapshot;
        s.users.retain(|u| keep.contains(&u.workspace));
        s.sections.retain(|x| keep.contains(&x.workspace));
        s.conversations.retain(|c| keep.contains(&c.workspace));
        if let Some(items) = s.saved.as_mut() {
            items.retain(|i| keep.contains(&i.workspace));
        }
        let mut old = std::mem::take(&mut s.workspaces);
        for (i, w) in signed.iter().enumerate() {
            let color = WorkspaceColor(u8::try_from(i).unwrap_or(u8::MAX));
            let placeholder = || Workspace { id: w.id.clone(), name: Remote::new(&w.name), color, me: UserId::new("") };
            let at = old.iter().position(|o| o.id == w.id);
            s.workspaces.push(at.map_or_else(placeholder, |at| Workspace { color, ..old.swap_remove(at) }));
        }
        for set in [&mut self.fresh, &mut self.cached, &mut self.saved_unknown] {
            set.retain(|w| keep.contains(w));
        }
        for w in signed {
            if !self.is_loaded_in(&w.id) {
                self.saved_unknown.insert(w.id.clone());
            }
        }
        self.index_users();
    }

    /// The boot answer of workspace `ws` alone (from the local cache when `cached`): its part
    /// of the model is replaced, the other workspaces' stay as they are. What the answer says
    /// of another workspace is not taken. Its place and color stay those it was listed with.
    pub fn merge(&mut self, ws: &WorkspaceId, snapshot: &Snapshot, cached: bool) {
        let new = snapshot.only(ws);
        let s = &mut self.snapshot;
        let at = s.workspaces.iter().position(|w| &w.id == ws);
        match (at, new.workspaces.into_iter().next()) {
            (Some(at), Some(w)) => s.workspaces[at] = Workspace { color: s.workspaces[at].color, ..w },
            (None, Some(w)) => s.workspaces.push(w),
            (_, None) => {}
        }
        s.users.retain(|u| &u.workspace != ws);
        s.users.extend(new.users);
        s.sections.retain(|x| &x.workspace != ws);
        s.sections.extend(new.sections);
        let mut conversations = new.conversations;
        if snapshot.counts_failed {
            // Counts Slack could not tell are unknown, not zero: those shown before stay.
            let before: HashMap<&ConversationId, (u32, u32)> = s
                .conversations
                .iter()
                .filter(|c| &c.workspace == ws)
                .map(|c| (&c.id, (c.unread, c.mentions)))
                .collect();
            for c in &mut conversations {
                if let Some(&(unread, mentions)) = before.get(&c.id) {
                    (c.unread, c.mentions) = (unread, mentions);
                }
            }
        }
        s.conversations.retain(|c| &c.workspace != ws);
        s.conversations.extend(conversations);
        let mut items = s.saved.take().unwrap_or_default();
        items.retain(|i| &i.workspace != ws);
        match new.saved {
            Some(mine) => {
                items.extend(mine);
                self.saved_unknown.remove(ws);
            }
            None => {
                self.saved_unknown.insert(ws.clone());
            }
        }
        s.saved = Some(items);
        s.taken = s.taken.max(new.taken);
        if cached {
            self.cached.insert(ws.clone());
        } else {
            self.cached.remove(ws);
            self.fresh.insert(ws.clone());
        }
        self.index_users();
    }

    /// Some workspace's data is here (from the backend or the cache).
    pub fn is_loaded(&self) -> bool {
        !self.fresh.is_empty() || !self.cached.is_empty()
    }

    /// Workspace `ws`'s data is here (from the backend or the cache).
    pub fn is_loaded_in(&self, ws: &WorkspaceId) -> bool {
        self.fresh.contains(ws) || self.cached.contains(ws)
    }

    /// Workspace `ws`'s backend answered (what the cache showed is replaced).
    pub fn is_fresh(&self, ws: &WorkspaceId) -> bool {
        self.fresh.contains(ws)
    }

    /// Something listed came from the local cache, not (yet) from the backend.
    pub fn is_cached(&self) -> bool {
        !self.cached.is_empty()
    }

    /// What workspace `ws` lists came from the local cache, not (yet) from the backend.
    pub fn is_cached_in(&self, ws: &WorkspaceId) -> bool {
        self.cached.contains(ws)
    }

    pub fn workspaces(&self) -> &[Workspace] {
        &self.snapshot.workspaces
    }

    /// Where workspace `ws` is in the switcher's order.
    pub fn position(&self, ws: &WorkspaceId) -> Option<usize> {
        self.snapshot.workspaces.iter().position(|w| &w.id == ws)
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

    /// The id of `row` (of the rows of this model), which outlives the row's index.
    pub fn key(&self, row: Row) -> Option<RowKey> {
        Some(match row {
            Row::Section(i) => RowKey::Section(self.snapshot.sections.get(i)?.id.clone()),
            Row::Conversation(i) => RowKey::Conversation(self.snapshot.conversations.get(i)?.id.clone()),
            Row::Group(g) => RowKey::Group(g),
            Row::Item(i) => RowKey::Item(self.items().get(i)?.id.clone()),
            Row::Filter => RowKey::Filter,
            Row::Spacer | Row::ItemMeta(_) => return None,
        })
    }

    /// People the backend learned of after the start (a page's authors, of workspace `ws`): each
    /// is added, or replaces what was known of them in `ws`; one of another workspace is left out.
    pub fn add_users(&mut self, ws: &WorkspaceId, users: &[User]) {
        for u in users.iter().filter(|u| &u.workspace == ws) {
            match self.users.get(&(u.workspace.clone(), u.id.clone())) {
                Some(&i) => self.snapshot.users[i] = u.clone(),
                None => {
                    self.users.insert((u.workspace.clone(), u.id.clone()), self.snapshot.users.len());
                    self.snapshot.users.push(u.clone());
                }
            }
        }
    }

    /// Person `id` as workspace `ws` lists them; when it does not (yet), as another workspace
    /// does (the same person: their name, not that workspace's view of their organization).
    pub fn user(&self, ws: &WorkspaceId, id: &UserId) -> Option<&User> {
        let i = self.users.get(&(ws.clone(), id.clone())).copied();
        let i = i.or_else(|| self.snapshot.users.iter().position(|u| &u.id == id));
        i.map(|i| &self.snapshot.users[i])
    }

    /// The name the model knows for a person or channel a message of `ws` names (sanitised).
    pub fn named(&self, ws: &WorkspaceId, piece: &crate::mrkdwn::Piece) -> Option<String> {
        use crate::mrkdwn::Piece;
        match piece {
            // A bot's author id of one name it posted as (`<bot>~<hash>`) is no person to
            // mention: a sender could make it name anyone.
            Piece::User { id, .. } if id.contains('~') => None,
            Piece::User { id, .. } => {
                self.user(ws, &UserId::new(id.as_str())).map(|u| u.display_name.line().into_string())
            }
            Piece::Channel { id, .. } => {
                let conversation = slakio_core::model::ConversationId::new(id.as_str());
                self.target(&Target::Conversation { workspace: ws.clone(), conversation })
                    .map(|c| c.name.line().into_string())
            }
            _ => None,
        }
    }

    pub fn workspace(&self, id: &WorkspaceId) -> Option<&Workspace> {
        self.snapshot.workspaces.iter().find(|w| &w.id == id)
    }

    /// The user's own account in workspace `id`.
    pub fn me(&self, id: &WorkspaceId) -> Option<&UserId> {
        self.workspace(id).map(|w| &w.me).filter(|me| !me.as_str().is_empty())
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
    fn todo_rows(&self, ws: &WorkspaceId, groups: &HashSet<Group>, opts: &TodoView) -> Vec<Row> {
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
        ws: &WorkspaceId,
        view: View,
        collapsed: &HashSet<SectionId>,
        groups: &HashSet<Group>,
        opts: &TodoView,
    ) -> Vec<Row> {
        if self.workspace(ws).is_none() {
            return vec![];
        }
        let convs = self.snapshot.conversations.iter().enumerate().filter(|(_, c)| &c.workspace == ws);
        match view {
            View::Home => {
                let mut rows = vec![];
                for (si, s) in self.snapshot.sections.iter().enumerate().filter(|(_, s)| &s.workspace == ws) {
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
        convs.fold((0, 0), |(u, m), c| (u + u32::from(c.badges().0 > 0), m + c.badges().1))
    }

    /// Unread mentions in workspace `ws`.
    pub fn workspace_mentions(&self, ws: &WorkspaceId) -> u32 {
        self.snapshot.conversations.iter().filter(|c| &c.workspace == ws).map(|c| c.badges().1).sum()
    }

    /// Workspace `ws` has an unread conversation that is not muted.
    pub fn workspace_unread(&self, ws: &WorkspaceId) -> bool {
        self.snapshot.conversations.iter().any(|c| &c.workspace == ws && c.badges().0 > 0)
    }

    /// Unread DM messages of workspace `ws`.
    pub fn dm_unread(&self, ws: &WorkspaceId) -> u32 {
        self.snapshot.conversations.iter().filter(|c| &c.workspace == ws && c.is_dm()).map(|c| c.badges().0).sum()
    }

    /// Unread mentions across every workspace (what Activity collects).
    pub fn mentions(&self) -> u32 {
        self.snapshot.conversations.iter().map(|c| c.badges().1).sum()
    }
}

#[cfg(test)]
mod tests;
