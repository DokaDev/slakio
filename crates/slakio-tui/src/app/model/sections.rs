//! A workspace's sidebar sections as Slack lists them, when they come after its boot answer.

use super::Model;
use slakio_core::model::{ConversationId, Section, SectionId, WorkspaceId};

impl Model {
    /// Slack's `sections` of workspace `ws` replace its own ([`slakio_core::backend::Snapshot::resection`]).
    /// `true` when the workspace is listed.
    pub fn resection(
        &mut self,
        ws: &WorkspaceId,
        sections: Vec<Section>,
        places: &[(ConversationId, SectionId)],
    ) -> bool {
        if self.workspace(ws).is_none() {
            return false;
        }
        self.snapshot.resection(ws, sections, places);
        true
    }
}
