//! The tabs and panes of the last run opened again ([`super::work`]'s `restore`): the binary
//! reads the saved layout at the start and hands it here. It opens once every pane's
//! conversation is listed (from the local cache, when there is one, so the panes show at once)
//! or known to be gone: a pane is left out only when its workspace's backend itself answered
//! without it, never on the cache's word, a failed or slow start, or no answer at all (the
//! status line says so once). Until then nothing is saved over the file, so a run that ends
//! early, or whose start fails, does not lose it. Tabs the user opened meanwhile stay as they
//! are, the saved ones after them.

use super::status::Level;
use super::{App, Focus};
use slakio_core::i18n::{Label, Msg};
use slakio_core::layout::saved::Saved;
use slakio_core::model::{Target, WorkspaceId};
use std::time::Instant;

impl App {
    /// The tabs of the last run, to open as soon as the workspaces have answered.
    pub fn restore_layout(&mut self, saved: Saved, now: Instant) {
        self.restoring = Some(saved);
        self.try_restore(now);
    }

    /// The saved layout could not be read: the app starts without it, and says so.
    pub fn layout_unreadable(&mut self, now: Instant) {
        self.status.queue(Msg::Label(Label::StatusLayoutUnreadable), Level::Warning, now);
    }

    /// What to save of the tabs now: `None` before there is a backend, and while the layout of
    /// the last run waits to be opened (the file keeps it).
    pub fn layout_to_save(&self) -> Option<Saved> {
        self.backend?;
        self.restoring.is_none().then(|| self.work.saved())
    }

    /// A number that moves whenever what [`App::layout_to_save`] gives may have changed, so the
    /// layout is written out only then.
    pub fn layout_stamp(&self) -> (u64, bool) {
        (self.work.edits(), self.backend.is_none() || self.restoring.is_some())
    }

    /// The backend's own word on workspace `ws` is in: its boot answered (not the cache's), or
    /// no boot of this run can ever list it.
    fn answered(&self, ws: &WorkspaceId) -> bool {
        if self.model.is_fresh(ws) {
            return true;
        }
        let own = self.boots.iter().any(|b| b.workspace.as_ref() == Some(ws));
        let all_at_once = self.boots.iter().any(|b| b.workspace.is_none());
        // One boot for every workspace: it answered when the list is the backend's own.
        !own && (!all_at_once || (self.model.is_loaded() && !self.model.is_cached()))
    }

    /// Pane target `t` can be opened, or is known to be gone: not waiting for an answer.
    fn settled(&self, t: &Target) -> bool {
        self.model.target(t).is_some() || self.answered(t.workspace())
    }

    /// Open the tabs waiting, once every pane's conversation is listed or known to be gone.
    /// `true` when the screen changed.
    pub(super) fn try_restore(&mut self, now: Instant) -> bool {
        let Some(saved) = &self.restoring else { return false };
        if self.boots.is_empty() || !saved.tabs.iter().flat_map(|t| &t.panes).all(|p| self.settled(&p.target)) {
            return false;
        }
        let Some(saved) = self.restoring.take() else { return false };
        let empty = self.work.tabs().is_empty();
        let model = &self.model;
        let dropped = self.work.restore(&saved, |t| model.target(t).is_some());
        if dropped > 0 {
            self.status.queue(Msg::Label(Label::StatusLayoutDropped), Level::Warning, now);
        }
        // What the user opened meanwhile keeps the keyboard.
        if let Some(id) = self.work.active().filter(|_| empty) {
            self.set_focus(Focus::on(id));
        }
        true
    }
}
