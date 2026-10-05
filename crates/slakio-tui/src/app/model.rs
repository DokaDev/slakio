//! The read model: what the backend said about the workspaces, people, sections and
//! conversations, with the queries the screen needs. Replaced as a whole by a boot answer; the
//! UI never changes it on its own.

use super::shell::View;
use slakio_core::backend::Snapshot;
use slakio_core::model::{Conversation, Section, SectionId, Target, Workspace};
use std::collections::HashSet;

/// One row of the list panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// A section header (index into the sections).
    Section(usize),
    /// A conversation (index into the conversations).
    Conversation(usize),
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    snapshot: Snapshot,
    /// Set once the backend answered.
    loaded: bool,
}

impl Model {
    pub fn new(snapshot: Snapshot) -> Self {
        Self { snapshot, loaded: true }
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

    /// The conversation `target` names.
    pub fn target(&self, target: &Target) -> Option<&Conversation> {
        match target {
            Target::Conversation { workspace, conversation } => {
                self.snapshot.conversations.iter().find(|c| &c.workspace == workspace && &c.id == conversation)
            }
        }
    }

    /// The rows of `view` in workspace `ws`, sections in `collapsed` folded.
    pub fn rows(&self, ws: usize, view: View, collapsed: &HashSet<SectionId>) -> Vec<Row> {
        let Some(w) = self.snapshot.workspaces.get(ws) else { return vec![] };
        let convs = self.snapshot.conversations.iter().enumerate().filter(|(_, c)| c.workspace == w.id);
        match view {
            View::Home => {
                let mut rows = vec![];
                for (si, s) in self.snapshot.sections.iter().enumerate().filter(|(_, s)| s.workspace == w.id) {
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
