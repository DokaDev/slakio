//! One backend of several workspaces: each signed-in workspace has a backend of its own (its own
//! sign-in, connection, cache), and this hands each request to the one it is about and every
//! answer back to the UI. A workspace whose backend fails, hangs or is gone holds none of the
//! others back: its requests fail on their own (`Offline` once it is gone, `Busy` while its queue
//! is full), and nothing waits on it.
//!
//! A request is about the workspace it names (`Request.workspace`), or a history request about
//! its target's. A request that names none, or one not signed in to, fails without going
//! anywhere. A change goes only to a workspace that can change things: each backend's own handle
//! refuses it for a read-only one. One workspace's backend can be replaced while the others run
//! on ([`Replace`]): signed in again, only that workspace starts over. Every request the old
//! backend had not answered then is answered once, as failed for the network (asking again may
//! help), and nothing more of the old backend is handed up.

use slakio_core::backend::{BackendHandle, Capabilities, Command, Envelope, Event, FailKind, Generation, Request};
use slakio_core::model::WorkspaceId;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

/// How many requests, and how many events, wait in the handle's channels at most.
pub const BOUND: usize = 64;

/// The most requests of one backend remembered as not answered yet (a backend may drop a
/// superseded request unanswered; the oldest are forgotten).
const ASKED_MAX: usize = 1024;

/// The workspace `request` is about, if it names one.
pub fn workspace_of(request: &Request) -> Option<&WorkspaceId> {
    match &request.command {
        Command::History { target, .. } | Command::Catchup { target, .. } => Some(target.workspace()),
        Command::Boot | Command::Save { .. } => request.workspace.as_ref(),
    }
}

/// The handle of the workspaces `backends`, each answering for its own (inside a Tokio
/// runtime). It can change something only where one of them can.
pub fn serve(backends: Vec<(WorkspaceId, BackendHandle)>) -> BackendHandle {
    serve_replaceable(backends).0
}

/// What puts another backend in place of one workspace's while the handle runs (signed in
/// again): the others go on as they were.
#[derive(Clone)]
pub struct Replace {
    served: Arc<Mutex<Vec<Served>>>,
    up: mpsc::Sender<Envelope>,
}

/// One workspace's backend as the handle serves it: where its requests go, and the task that
/// hands its answers up (ended when it is replaced, so nothing of it is kept alive).
struct Served {
    ws: WorkspaceId,
    backend: BackendHandle,
    answers: AbortHandle,
    asked: Arc<Mutex<Asked>>,
}

/// A backend's requests not answered yet; `replaced` once another backend is in its place
/// (nothing more of it is handed up then).
#[derive(Default)]
struct Asked {
    ids: BTreeSet<Generation>,
    replaced: bool,
}

/// The answer that ends request `generation`'s wait (not one of the cache's, given first, nor
/// a live update).
fn ends(event: &Event) -> bool {
    matches!(event, Event::Booted(_) | Event::History(_) | Event::Saved { .. } | Event::Failed { .. })
}

impl Replace {
    /// Workspace `ws` is served.
    pub fn serves(&self, ws: &WorkspaceId) -> bool {
        self.served.lock().unwrap_or_else(PoisonError::into_inner).iter().any(|s| &s.ws == ws)
    }

    /// Serve `ws` with `backend` from now on, the one before dropped (its requests end, its late
    /// answers are not handed up; each it had not answered is answered as failed for the
    /// network, once). `false`, and nothing changes, when `ws` is not served.
    pub fn put(&self, ws: &WorkspaceId, backend: BackendHandle) -> bool {
        let mut served = self.served.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(s) = served.iter_mut().find(|s| &s.ws == ws) else { return false };
        let unanswered = {
            let mut asked = s.asked.lock().unwrap_or_else(PoisonError::into_inner);
            asked.replaced = true;
            std::mem::take(&mut asked.ids)
        };
        s.answers.abort();
        let asked = Arc::default();
        let (backend, answers) = forward(backend, self.up.clone(), Arc::clone(&asked));
        (s.backend, s.answers, s.asked) = (backend, answers, asked);
        let up = self.up.clone();
        tokio::spawn(async move {
            for id in unanswered {
                if up.send(Envelope::failed(id, FailKind::Network, "signed in again")).await.is_err() {
                    return;
                }
            }
        });
        true
    }
}

/// Hand `backend`'s answers to `up` on a task of their own (a slow workspace never delays
/// another's), each request's answer once, and none once `asked` says it was replaced: its
/// handle for requests, and that task.
fn forward(
    backend: BackendHandle,
    up: mpsc::Sender<Envelope>,
    asked: Arc<Mutex<Asked>>,
) -> (BackendHandle, AbortHandle) {
    let BackendHandle { capabilities, tx, mut rx } = backend;
    let task = tokio::spawn(async move {
        while let Some(envelope) = rx.recv().await {
            let Ok(room) = up.reserve().await else { return };
            // Looked at once there is room, then handed up at once: no answer is lost or doubled
            // by a replacement in between.
            let mut a = asked.lock().unwrap_or_else(PoisonError::into_inner);
            if a.replaced {
                return;
            }
            if !ends(&envelope.event) || a.ids.remove(&envelope.generation) {
                room.send(envelope);
            }
        }
    });
    let (_, unused) = mpsc::channel(1);
    (BackendHandle { capabilities, tx, rx: unused }, task.abort_handle())
}

/// [`serve`], and what replaces one workspace's backend later.
pub fn serve_replaceable(backends: Vec<(WorkspaceId, BackendHandle)>) -> (BackendHandle, Replace) {
    let caps = Capabilities {
        demo: backends.iter().any(|(_, b)| b.capabilities.demo),
        read_only: backends.iter().all(|(_, b)| b.capabilities.read_only),
    };
    let (handle, mut end) = BackendHandle::pair(caps, BOUND);
    let served: Vec<Served> = backends
        .into_iter()
        .map(|(ws, b)| {
            let asked = Arc::default();
            let (backend, answers) = forward(b, end.tx.clone(), Arc::clone(&asked));
            Served { ws, backend, answers, asked }
        })
        .collect();
    let replace = Replace { served: Arc::new(Mutex::new(served)), up: end.tx.clone() };
    let served = Arc::clone(&replace.served);
    tokio::spawn(async move {
        while let Some(request) = end.rx.recv().await {
            let refused = {
                let served = served.lock().unwrap_or_else(PoisonError::into_inner);
                match workspace_of(&request).and_then(|ws| served.iter().find(|s| &s.ws == ws)) {
                    Some(s) => {
                        let id = request.id;
                        let mut asked = s.asked.lock().unwrap_or_else(PoisonError::into_inner);
                        asked.ids.insert(id);
                        while asked.ids.len() > ASKED_MAX {
                            asked.ids.pop_first();
                        }
                        let refused = s.backend.send(request).err();
                        if refused.is_some() {
                            asked.ids.remove(&id);
                        }
                        refused
                    }
                    None => {
                        Some(Envelope::failed(request.id, FailKind::Auth, "no workspace signed in for this request"))
                    }
                }
            };
            if let Some(failed) = refused
                && end.tx.send(failed).await.is_err()
            {
                return;
            }
        }
    });
    (handle, replace)
}
