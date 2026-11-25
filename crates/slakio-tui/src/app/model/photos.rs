//! Photos no one shows any more: a person's new photo replaced the old one, or Slack said they
//! have none of their own. The app lets the terminal drop those pictures
//! ([`crate::images::Images::forget_photo`]), so none stays held for a photo that is gone.

use super::Model;
use slakio_core::backend::Snapshot;
use std::collections::HashSet;

impl Model {
    /// Person's photo `old` is `now` instead: `old` is noted as gone when it changed.
    pub(super) fn photo_changed(&mut self, old: Option<String>, now: Option<&String>) {
        if let Some(old) = old.filter(|o| now != Some(o)) {
            self.gone_photos.push(old);
        }
    }

    /// The photos noted as gone since this was last asked, each once, but those someone still
    /// shows (a bot's names share its picture).
    pub fn take_gone_photos(&mut self) -> Vec<String> {
        let mut gone = std::mem::take(&mut self.gone_photos);
        gone.sort_unstable();
        gone.dedup();
        gone.retain(|a| !self.snapshot.users.iter().any(|u| u.avatar.as_ref() == Some(a)));
        gone
    }

    /// The model of `snapshot` (a whole new boot), each photo shown now kept for a person it says
    /// nothing of ([`Model::with_photos`]); those it shows for no one are noted as gone.
    pub fn renewed(&self, snapshot: Snapshot) -> Model {
        let next = self.with_photos(snapshot);
        let kept: HashSet<&String> = next.users.iter().filter_map(|u| u.avatar.as_ref()).collect();
        let gone = self.snapshot.users.iter().filter_map(|u| u.avatar.clone()).filter(|a| !kept.contains(a));
        let gone = gone.collect();
        Model { gone_photos: gone, ..Model::new(next) }
    }
}
