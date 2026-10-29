//! A workspace signed in to again as someone else: nothing of the user before stays.

use super::Model;
use slakio_core::model::{UserId, WorkspaceId};

impl Model {
    /// Forget everything workspace `ws` showed — its people, sections, conversations (their
    /// counts with them), saved items and live updates — until its next boot answers; it stays
    /// listed, by its name, in its place.
    pub fn forget_workspace(&mut self, ws: &WorkspaceId) {
        let s = &mut self.snapshot;
        s.users.retain(|u| &u.workspace != ws);
        s.sections.retain(|x| &x.workspace != ws);
        s.conversations.retain(|c| &c.workspace != ws);
        if let Some(items) = s.saved.as_mut() {
            items.retain(|i| &i.workspace != ws);
        }
        if let Some(w) = s.workspaces.iter_mut().find(|w| &w.id == ws) {
            w.me = UserId::new("");
        }
        self.fresh.remove(ws);
        self.cached.remove(ws);
        self.saved_unknown.insert(ws.clone());
        self.counts_unknown.remove(ws);
        self.bot_names.retain(|(w, _), _| w != ws);
        self.live.remove(ws);
        self.index_users();
    }
}
