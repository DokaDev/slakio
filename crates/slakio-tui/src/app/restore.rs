//! The tabs and panes of the last run opened again ([`super::work`]'s `restore`): the binary
//! reads the saved layout at the start and hands it here; it is opened once every workspace has
//! answered its boot (from the local cache, when there is one, so the panes show at once), and
//! only into an empty work area (what the user opened first wins). A pane whose conversation
//! the workspaces no longer list is left out, and the status line says so once. Until then, what
//! is saved is the layout still waiting, so a run that ends early does not lose it.

use super::status::Level;
use super::{App, Focus};
use slakio_core::i18n::{Label, Msg};
use slakio_core::layout::saved::Saved;
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

    /// What to save of the tabs now: `None` before there is a backend (nothing to keep); the
    /// layout of the last run while it waits to be opened.
    pub fn layout_to_save(&self) -> Option<Saved> {
        self.backend?;
        Some(self.restoring.clone().unwrap_or_else(|| self.work.saved()))
    }

    /// Every workspace answered its boot, or failed to.
    fn booted(&self) -> bool {
        let ws = self.model.workspaces();
        let loaded = |id| self.model.is_loaded_in(id);
        !self.boots.is_empty()
            && self.boots.iter().all(|b| {
                b.failed.is_some()
                    || match &b.workspace {
                        Some(id) => loaded(id),
                        None => !ws.is_empty() && ws.iter().all(|w| loaded(&w.id)),
                    }
            })
    }

    /// Open the tabs waiting, once the workspaces answered. `true` when the screen changed.
    pub(super) fn try_restore(&mut self, now: Instant) -> bool {
        if self.restoring.is_none() || !self.booted() {
            return false;
        }
        let Some(saved) = self.restoring.take() else { return false };
        if !self.work.tabs().is_empty() {
            return false;
        }
        let model = &self.model;
        let dropped = self.work.restore(&saved, |t| model.target(t).is_some());
        if dropped > 0 {
            self.status.queue(Msg::Label(Label::StatusLayoutDropped), Level::Warning, now);
        }
        if let Some(id) = self.work.active() {
            self.set_focus(Focus::on(id));
        }
        true
    }
}
