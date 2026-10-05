//! The read model: what the backend said about the workspaces, people, sections and
//! conversations, with the queries the screen needs. Replaced as a whole by a boot answer; the
//! UI never changes it on its own.

use super::shell::View;
use slakio_core::backend::Snapshot;
use slakio_core::model::{Conversation, Section, SectionId, Target, User, UserId, Workspace, WorkspaceId};
use std::collections::{HashMap, HashSet};

/// One row of the list panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// A section header (index into the sections).
    Section(usize),
    /// A conversation (index into the conversations).
    Conversation(usize),
    /// The blank row between two sections: never under the cursor.
    Spacer,
}

impl Row {
    /// The cursor can stop here.
    pub fn is_selectable(self) -> bool {
        self != Row::Spacer
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

    /// The rows of `view` in workspace `ws`, sections in `collapsed` folded.
    pub fn rows(&self, ws: usize, view: View, collapsed: &HashSet<SectionId>) -> Vec<Row> {
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
                rows
            }
            View::Dms => convs.filter(|(_, c)| c.is_dm()).map(|(i, _)| Row::Conversation(i)).collect(),
            // Not built yet: the list panel says so.
            View::Activity | View::Files | View::Later => vec![],
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
