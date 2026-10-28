//! The backend as the app sees it: connecting asks for the workspaces, the requests queued are
//! taken by the binary's loop, and each answer goes to whoever asked ([`App::on_backend`]). A
//! failed request ends what waited for it — the list's "Loading", a page asked for, a change to
//! a saved item — and the status line says why; nothing waits for an answer that never comes.

use super::model::{Model, SignedIn};
use super::status::Level;
use super::{App, Effect};
use slakio_core::backend::{Capabilities, Command, Envelope, Event, FailKind, Generation, Request, Snapshot};
use slakio_core::fault::Fault;
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::WorkspaceId;
use std::time::Instant;

/// A boot asked for: of one workspace, or of every workspace the backend has (`None`).
#[derive(Clone, Debug)]
pub(crate) struct Boot {
    pub workspace: Option<WorkspaceId>,
    /// The last request (an answer to an older one is dropped).
    pub id: Generation,
    /// Why it failed, when it did.
    pub failed: Option<FailKind>,
    /// Nothing can be changed in the workspace.
    pub read_only: bool,
}

impl App {
    /// Use a backend that can do `caps`: asks it for every workspace at once.
    pub fn connect(&mut self, caps: Capabilities) {
        self.backend = Some(caps);
        let id = self.work.requests.ask(None, Command::Boot);
        self.boots = vec![Boot { workspace: None, id, failed: None, read_only: caps.read_only }];
    }

    /// Use a backend of the workspaces `signed` (signed in to, in the switcher's order), each
    /// asked for on its own: one that fails or never answers holds none of the others back.
    pub fn connect_workspaces(&mut self, caps: Capabilities, signed: &[SignedIn]) {
        self.backend = Some(caps);
        self.model.sign_ins(signed);
        self.shell.clamp(&self.model, self.list_height());
        self.work.clamp(&self.model);
        self.boots = signed
            .iter()
            .map(|s| Boot {
                workspace: Some(s.id.clone()),
                id: self.work.requests.ask(Some(s.id.clone()), Command::Boot),
                failed: None,
                read_only: caps.read_only || s.read_only,
            })
            .collect();
    }

    /// The requests for the backend queued since the last call.
    pub fn take_commands(&mut self, now: Instant) -> Vec<Request> {
        self.work.send_older(now);
        self.work.take_requests()
    }

    /// The boot of the workspace shown (or of every workspace).
    fn boot_here(&self) -> Option<&Boot> {
        let ws = self.shell.current_id(&self.model);
        let own = self.boots.iter().find(|b| b.workspace.is_some() && b.workspace == ws);
        own.or_else(|| self.boots.iter().find(|b| b.workspace.is_none()))
    }

    /// Nothing can be changed in the workspace shown: no change is offered.
    pub fn read_only(&self) -> bool {
        self.backend.is_some_and(|c| c.read_only) || self.boot_here().is_some_and(|b| b.read_only)
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
    /// the list of the workspace shown, or the active pane's messages.
    pub fn showing_cached(&self) -> bool {
        self.shell.current(&self.model).is_some_and(|w| self.model.is_cached_in(&w.id)) || self.work.active_cached()
    }

    /// Why the last request for the workspace shown failed, if it did.
    pub fn boot_failed(&self) -> Option<&FailKind> {
        self.boot_here()?.failed.as_ref()
    }

    /// The workspace shown boots on its own (not with every workspace at once).
    pub fn boots_alone(&self) -> bool {
        self.boot_here().is_some_and(|b| b.workspace.is_some())
    }

    /// Why nothing of the workspace shown could be loaded, in words, when asking again cannot
    /// help and it is one of several (with one, the sign-in screen says it): its list says why,
    /// and what to do, for as long as it shows, once the notice is gone.
    pub fn failed_why(&self) -> Option<Label> {
        let ws = self.shell.current_id(&self.model)?;
        let kind = self.boot_failed()?;
        let shown = self.model.is_loaded_in(&ws);
        (self.boots.len() > 1 && self.boots_alone() && !shown && !kind.retryable()).then(|| failure(kind))
    }

    /// Why the last request for workspace `ws` failed, if it did.
    pub fn boot_failed_in(&self, ws: &WorkspaceId) -> Option<&FailKind> {
        let own = self.boots.iter().find(|b| b.workspace.as_ref() == Some(ws));
        own.or_else(|| self.boots.iter().find(|b| b.workspace.is_none()))?.failed.as_ref()
    }

    /// What could not be loaded where retrying would ask again — the workspace shown, else the
    /// active pane's messages — and why; `None` when nothing failed there.
    pub fn failed_here(&self) -> Option<&FailKind> {
        self.boot_failed().or_else(|| self.work.active_failed())
    }

    /// `r` (`:retry`): ask again for what could not be loaded for a reason that may pass — the
    /// workspace shown (also one still loading, or shown from the cache while Slack has not
    /// answered: its backend may hang or be gone), else the active pane's messages. Only when
    /// asked: nothing retries on its own.
    pub(super) fn retry(&mut self, now: Instant) {
        let here = self.boot_here().map(|b| (b.workspace.clone(), b.failed.clone()));
        let loading = matches!(&here, Some((Some(ws), None)) if !self.model.is_fresh(ws));
        match here {
            Some((ws, Some(kind))) if kind.retryable() => self.boot_again(ws),
            // Asking again cannot help: say why once more.
            Some((_, Some(kind))) => self.warn(Msg::Label(failure(&kind)), now),
            // An answer to the older ask, should it still come, is dropped.
            Some((ws, None)) if loading => self.boot_again(ws),
            _ if self.work.retry() => {}
            _ => self.info(Msg::Label(Label::StatusNothingToRetry), now),
        }
    }

    /// Ask again for the boot of `ws`: the answer to the one asked before is dropped.
    fn boot_again(&mut self, ws: Option<WorkspaceId>) {
        let id = self.work.requests.ask(ws.clone(), Command::Boot);
        if let Some(b) = self.boots.iter_mut().find(|b| b.workspace == ws) {
            (b.id, b.failed) = (id, None);
        }
    }

    /// An event of the backend, which arrived at `now`. `true` when the screen changed; an
    /// answer to an older request is dropped.
    pub fn on_backend(&mut self, envelope: Envelope, now: Instant) -> bool {
        let id = envelope.generation;
        let boot = self.boots.iter().position(|b| b.id == id);
        match envelope.event {
            Event::Booted(snapshot) => boot.is_some_and(|b| self.on_boot(b, &snapshot, false, now)),
            Event::CachedBoot(snapshot) => boot.is_some_and(|b| self.on_boot(b, &snapshot, true, now)),
            Event::CachedHistory(page) => {
                self.model.add_users(page.target.workspace(), &page.users);
                self.work.on_cached_page(id, &page, &self.model)
            }
            Event::History(page) => {
                // Its authors' names first: a page's messages are drawn with them as it arrives.
                self.model.add_users(page.target.workspace(), &page.users);
                self.work.on_page(id, &page, &self.model)
            }
            Event::Saved { items, taken } => self.on_saved(id, items, taken, now),
            Event::Failed { request, kind, detail } => self.on_failed(request, kind, &detail, now),
        }
    }

    /// The answer of boot `b` (the cache's, when `cached`: drawn only while the backend's own
    /// has not come). The list's cursor stays on the row it was on.
    fn on_boot(&mut self, b: usize, snapshot: &Snapshot, cached: bool, now: Instant) -> bool {
        let anchor = self.shell.anchor(&self.model);
        match self.boots[b].workspace.clone() {
            None if !cached => self.model = Model::new(snapshot.clone()),
            None if !self.model.is_loaded() || self.model.is_cached() => self.model = Model::cached(snapshot.clone()),
            Some(ws) if !cached || !self.model.is_fresh(&ws) => self.model.merge(&ws, snapshot, cached),
            _ => return false,
        }
        if !cached {
            self.boots[b].failed = None;
            // Unknown is not zero: the counts kept are shown, and the status line says so (with
            // several workspaces, whose; its stripe found by id, two may have one name).
            if snapshot.counts_failed {
                let whose = self.boots[b].workspace.clone().filter(|_| self.boots.len() > 1);
                match whose.and_then(|ws| self.model.workspace(&ws).cloned()) {
                    Some(w) => {
                        let why = self.i18n.label(Label::FailCounts).to_string();
                        let msg = Msg::FailWorkspace { workspace: w.name.line().into_string(), why };
                        self.status.queue_about(msg, Level::Warning, Some(w.id.clone()), now);
                    }
                    None => self.status.queue(Msg::Label(Label::FailCounts), Level::Warning, now),
                }
            }
        }
        let h = self.list_height();
        self.shell.clamp(&self.model, h);
        self.shell.restore(anchor, &self.model, h);
        self.work.clamp(&self.model);
        true
    }

    /// Request `id` failed: whatever waited for it stops waiting, the status line says why and
    /// the error log gets the backend's detail.
    fn on_failed(&mut self, id: Generation, kind: FailKind, detail: &str, now: Instant) -> bool {
        let mut whose = None;
        let waited = if let Some(b) = self.boots.iter_mut().find(|b| b.id == id) {
            b.failed = Some(kind.clone());
            whose = b.workspace.clone();
            true
        } else {
            self.work.on_failed(id, kind.clone(), now) || self.on_save_failed(id)
        };
        if waited {
            // With several workspaces, the words say whose start failed.
            let name = whose.filter(|_| self.boots.len() > 1).and_then(|ws| self.model.workspace(&ws).cloned());
            // A notice about a workspace names it by id, so its stripe is its own whatever its name.
            let about = |w: &slakio_core::model::Workspace| Some(w.id.clone());
            match name {
                // A refused sign-in has words of its own, short enough to keep the name.
                Some(w) if kind == FailKind::Auth => {
                    let msg = Msg::FailWorkspaceAuth { workspace: w.name.line().into_string() };
                    self.status.show_about(msg, Level::Warning, about(&w), now);
                }
                Some(w) => {
                    let why = self.i18n.label(failure(&kind)).to_string();
                    let msg = Msg::FailWorkspace { workspace: w.name.line().into_string(), why };
                    self.status.show_about(msg, Level::Warning, about(&w), now);
                }
                None => self.warn(Msg::Label(failure(&kind)), now),
            }
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
