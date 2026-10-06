//! What is being written to each conversation and thread: one composer per [`Target`], shared
//! by the panes that show it and kept when they close, so a draft is there again when its
//! target is opened again (for as long as the app runs).

use super::composer::Composer;
use slakio_core::model::Target;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct DraftStore {
    drafts: HashMap<Target, Composer>,
}

impl DraftStore {
    pub fn get(&self, target: &Target) -> Option<&Composer> {
        self.drafts.get(target)
    }

    /// The draft of `target`, empty if it had none.
    pub fn entry(&mut self, target: &Target) -> &mut Composer {
        self.drafts.entry(target.clone()).or_default()
    }

    /// Keep only the drafts `keep` says yes to.
    pub fn retain(&mut self, mut keep: impl FnMut(&Target, &Composer) -> bool) {
        self.drafts.retain(|t, c| keep(t, c));
    }

    /// Any draft holds text that was not sent.
    pub fn unsent(&self) -> bool {
        self.drafts.values().any(|c| !c.text().trim().is_empty())
    }
}
