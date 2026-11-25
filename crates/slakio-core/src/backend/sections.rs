//! A workspace's sidebar sections in a snapshot: Slack's, when they come after its boot
//! ([`super::Live::Sections`]), and those known before, when a boot could not read them
//! ([`Snapshot::sections_unknown`]).

use super::Snapshot;
use crate::model::{ConversationId, Section, SectionId, WorkspaceId};
use std::collections::{HashMap, HashSet};

impl Snapshot {
    /// `sections` (those of workspace `ws`) in place of the workspace's own, where its own were
    /// among the other workspaces'; each conversation of `ws` that `places` names goes to its
    /// section, and one it does not name, or names in a section not listed, stays where it is.
    pub fn resection(&mut self, ws: &WorkspaceId, sections: Vec<Section>, places: &[(ConversationId, SectionId)]) {
        let sections: Vec<Section> = sections.into_iter().filter(|x| &x.workspace == ws).collect();
        let listed: HashSet<&SectionId> = sections.iter().map(|x| &x.id).collect();
        let places: HashMap<&ConversationId, SectionId> =
            places.iter().filter(|(_, at)| listed.contains(at)).map(|(c, at)| (c, at.clone())).collect();
        for c in self.conversations.iter_mut().filter(|c| &c.workspace == ws) {
            if let Some(at) = places.get(&c.id) {
                c.section = at.clone();
            }
        }
        let at = self.sections.iter().position(|x| &x.workspace == ws).unwrap_or(self.sections.len());
        self.sections.retain(|x| &x.workspace != ws);
        let at = at.min(self.sections.len());
        self.sections.splice(at..at, sections);
    }

    /// Workspace `ws`'s sections of `before` kept in place of this snapshot's (which does not
    /// know Slack's): this snapshot's own sections `before` does not have are added after them,
    /// and each conversation `before` lists stays in its section. Nothing changes when `before`
    /// has no section of `ws`.
    pub fn keep_sections(&mut self, ws: &WorkspaceId, before: &Snapshot) {
        let mut kept: Vec<Section> = before.sections.iter().filter(|x| &x.workspace == ws).cloned().collect();
        if kept.is_empty() {
            return;
        }
        let known: HashSet<SectionId> = kept.iter().map(|x| x.id.clone()).collect();
        kept.extend(self.sections.iter().filter(|x| &x.workspace == ws && !known.contains(&x.id)).cloned());
        let places: Vec<(ConversationId, SectionId)> = (before.conversations.iter())
            .filter(|c| &c.workspace == ws)
            .map(|c| (c.id.clone(), c.section.clone()))
            .collect();
        self.resection(ws, kept, &places);
    }
}
