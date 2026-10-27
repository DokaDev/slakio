//! The backend as the app sees it: connecting asks for the workspaces, the requests queued are
//! taken by the binary's loop, and each answer goes to whoever asked ([`App::on_backend`]). A
//! failed request ends what waited for it — the list's "Loading", a page asked for, a change to
//! a saved item — and the status line says why; nothing waits for an answer that never comes.

use super::model::Model;
use super::{App, Effect};
use slakio_core::backend::{Capabilities, Command, Envelope, Event, FailKind, Generation, Request};
use slakio_core::fault::Fault;
use slakio_core::i18n::{Label, Msg};
use std::time::Instant;

impl App {
    /// Use a backend that can do `caps`: asks it for the workspaces.
    pub fn connect(&mut self, caps: Capabilities) {
        self.backend = Some(caps);
        self.boot = (self.work.requests.ask(None, Command::Boot), None);
    }

    /// The requests for the backend queued since the last call.
    pub fn take_commands(&mut self) -> Vec<Request> {
        self.work.take_requests()
    }

    /// The backend changes nothing: no change is offered.
    pub fn read_only(&self) -> bool {
        self.backend.is_some_and(|c| c.read_only)
    }

    /// Write in the focused pane's composer; a read-only backend sends nothing, so it says so
    /// instead.
    pub(super) fn write(&mut self, now: Instant) {
        if self.read_only() {
            self.info(Msg::Label(Label::ComposerReadOnly), now);
        } else if self.work.focused().is_some() {
            self.work.set_insert(true);
        }
    }

    /// What is shown came from the local cache and the backend has not answered with its own:
    /// the list, or the active pane's messages.
    pub fn showing_cached(&self) -> bool {
        self.model.is_cached() || self.work.active_cached()
    }

    /// Why the last request for the workspaces failed, if it did.
    pub fn boot_failed(&self) -> Option<&FailKind> {
        self.boot.1.as_ref()
    }

    /// What could not be loaded where retrying would ask again — the workspaces, else the active
    /// pane's messages — and why; `None` when nothing failed there.
    pub fn failed_here(&self) -> Option<&FailKind> {
        self.boot.1.as_ref().or_else(|| self.work.active_failed())
    }

    /// `r` (`:retry`): ask again for what could not be loaded for a reason that may pass — the
    /// workspaces, else the active pane's messages. Only when asked: nothing retries on its own.
    pub(super) fn retry(&mut self, now: Instant) {
        match &self.boot.1 {
            Some(kind) if kind.retryable() => self.boot = (self.work.requests.ask(None, Command::Boot), None),
            // Asking again cannot help: say why once more.
            Some(kind) => self.warn(Msg::Label(failure(kind)), now),
            None if self.work.retry() => {}
            None => self.info(Msg::Label(Label::StatusNothingToRetry), now),
        }
    }

    /// An event of the backend, which arrived at `now`. `true` when the screen changed; an
    /// answer to an older request is dropped.
    pub fn on_backend(&mut self, envelope: Envelope, now: Instant) -> bool {
        let id = envelope.generation;
        match envelope.event {
            Event::Booted(snapshot) if id == self.boot.0 => {
                self.model = Model::new(*snapshot);
                self.boot.1 = None;
                self.shell.clamp(&self.model, self.list_height());
                self.work.clamp(&self.model);
                true
            }
            Event::Booted(_) => false,
            // The cache's snapshot draws only while the backend's own has not come.
            Event::CachedBoot(snapshot) if id == self.boot.0 && (!self.model.is_loaded() || self.model.is_cached()) => {
                self.model = Model::cached(*snapshot);
                self.shell.clamp(&self.model, self.list_height());
                self.work.clamp(&self.model);
                true
            }
            Event::CachedBoot(_) => false,
            Event::CachedHistory(page) => {
                self.model.add_users(&page.users);
                self.work.on_cached_page(id, &page, &self.model)
            }
            Event::History(page) => {
                // Its authors' names first: a page's messages are drawn with them as it arrives.
                self.model.add_users(&page.users);
                self.work.on_page(id, &page, &self.model)
            }
            Event::Saved { items, taken } => self.on_saved(id, items, taken, now),
            Event::Failed { request, kind, detail } => self.on_failed(request, kind, &detail, now),
        }
    }

    /// Request `id` failed: whatever waited for it stops waiting, the status line says why and
    /// the error log gets the backend's detail.
    fn on_failed(&mut self, id: Generation, kind: FailKind, detail: &str, now: Instant) -> bool {
        let waited = if id == self.boot.0 {
            self.boot.1 = Some(kind.clone());
            true
        } else {
            self.work.on_failed(id, kind.clone()) || self.on_save_failed(id)
        };
        if waited {
            self.warn(Msg::Label(failure(&kind)), now);
            let fault = Fault::other(format!("{kind:?}: {}", log_safe(detail)));
            self.effects.push(Effect::Record { context: "backend", fault });
        }
        waited
    }
}

/// The status line's words for a failure of `kind`.
pub(super) fn failure(kind: &FailKind) -> Label {
    match kind {
        FailKind::ReadOnly => Label::FailReadOnly,
        FailKind::Network => Label::FailNetwork,
        FailKind::Auth => Label::FailAuth,
        FailKind::RateLimited { .. } => Label::FailRateLimited,
        FailKind::CapReached => Label::FailCapReached,
        FailKind::Offline => Label::FailOffline,
        FailKind::Busy => Label::FailBusy,
        FailKind::Api { .. } => Label::FailApi,
    }
}

/// The most of a backend's detail the error log keeps.
const DETAIL_MAX: usize = 200;

/// `detail` as one line of the error log: control characters (a terminal escape among them)
/// become spaces, and it is cut to [`DETAIL_MAX`] characters. An adapter puts no message text,
/// token or cookie in a detail; this only keeps a stray one from growing the log.
fn log_safe(detail: &str) -> String {
    detail.chars().map(|c| if c.is_control() { ' ' } else { c }).take(DETAIL_MAX).collect()
}
