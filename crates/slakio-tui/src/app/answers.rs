//! The backend as the app sees it: connecting asks for the workspaces, the requests queued are
//! taken by the binary's loop, and each answer goes to whoever asked ([`App::on_backend`]). A
//! failed request ends what waited for it — the list's "Loading", a page asked for, a change to
//! a saved item — and the status line says why; nothing waits for an answer that never comes.

use super::App;
use super::model::Model;
use slakio_core::backend::{Capabilities, Command, Envelope, Event, FailKind, Generation, Request};
use slakio_core::i18n::{Label, Msg};
use std::time::Instant;

impl App {
    /// Use a backend that can do `caps`: asks it for the workspaces.
    pub fn connect(&mut self, caps: Capabilities) {
        self.backend = Some(caps);
        self.boot = (self.work.requests.ask(None, Command::Boot), false);
    }

    /// The requests for the backend queued since the last call.
    pub fn take_commands(&mut self) -> Vec<Request> {
        self.work.take_requests()
    }

    /// The backend changes nothing: no change is offered.
    pub fn read_only(&self) -> bool {
        self.backend.is_some_and(|c| c.read_only)
    }

    /// The last request for the workspaces failed.
    pub fn boot_failed(&self) -> bool {
        self.boot.1
    }

    /// An event of the backend, which arrived at `now`. `true` when the screen changed; an
    /// answer to an older request is dropped.
    pub fn on_backend(&mut self, envelope: Envelope, now: Instant) -> bool {
        let id = envelope.generation;
        match envelope.event {
            Event::Booted(snapshot) if id == self.boot.0 => {
                self.model = Model::new(*snapshot);
                self.boot.1 = false;
                self.shell.clamp(&self.model, self.list_height());
                self.work.clamp(&self.model);
                true
            }
            Event::Booted(_) => false,
            Event::History(page) => self.work.on_page(id, &page, &self.model),
            Event::Saved { items, taken } => self.on_saved(id, items, taken, now),
            Event::Failed { request, kind, .. } => self.on_failed(request, kind, now),
        }
    }

    /// Request `id` failed: whatever waited for it stops waiting, and the status line says why.
    fn on_failed(&mut self, id: Generation, kind: FailKind, now: Instant) -> bool {
        let waited = if id == self.boot.0 {
            self.boot.1 = true;
            true
        } else {
            self.work.on_failed(id) || self.on_save_failed(id)
        };
        if waited {
            self.warn(Msg::Label(failure(kind)), now);
        }
        waited
    }
}

/// The status line's words for a failure of `kind`.
pub(super) fn failure(kind: FailKind) -> Label {
    match kind {
        FailKind::ReadOnly => Label::FailReadOnly,
        FailKind::Network => Label::FailNetwork,
        FailKind::Auth => Label::FailAuth,
        FailKind::RateLimited => Label::FailRateLimited,
        FailKind::CapReached => Label::FailCapReached,
        FailKind::Offline => Label::FailOffline,
        FailKind::Busy => Label::FailBusy,
    }
}
