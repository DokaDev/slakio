//! One backend of several workspaces: each signed-in workspace has a backend of its own (its own
//! sign-in, connection, cache), and this hands each request to the one it is about and every
//! answer back to the UI. A workspace whose backend fails, hangs or is gone holds none of the
//! others back: its requests fail on their own (`Offline` once it is gone, `Busy` while its queue
//! is full), and nothing waits on it.
//!
//! A request is about the workspace it names (`Request.workspace`), or a history request about
//! its target's. A request that names none, or one not signed in to, fails without going
//! anywhere. A change goes only to a workspace that can change things: each backend's own handle
//! refuses it for a read-only one.

use slakio_core::backend::{BackendHandle, Capabilities, Command, Envelope, FailKind, Request};
use slakio_core::model::WorkspaceId;

/// How many requests, and how many events, wait in the handle's channels at most.
pub const BOUND: usize = 64;

/// The workspace `request` is about, if it names one.
pub fn workspace_of(request: &Request) -> Option<&WorkspaceId> {
    match &request.command {
        Command::History { target, .. } => Some(target.workspace()),
        Command::Boot | Command::Save { .. } => request.workspace.as_ref(),
    }
}

/// The handle of the workspaces `backends`, each answering for its own (inside a Tokio
/// runtime). It can change something only where one of them can.
pub fn serve(backends: Vec<(WorkspaceId, BackendHandle)>) -> BackendHandle {
    let caps = Capabilities {
        demo: backends.iter().any(|(_, b)| b.capabilities.demo),
        read_only: backends.iter().all(|(_, b)| b.capabilities.read_only),
    };
    let (handle, mut end) = BackendHandle::pair(caps, BOUND);
    let mut senders = Vec::with_capacity(backends.len());
    for (ws, b) in backends {
        let BackendHandle { capabilities, tx, mut rx } = b;
        let up = end.tx.clone();
        // Each workspace's answers on their own task: a slow one never delays another's.
        tokio::spawn(async move {
            while let Some(envelope) = rx.recv().await {
                if up.send(envelope).await.is_err() {
                    return;
                }
            }
        });
        let (_, unused) = tokio::sync::mpsc::channel(1);
        senders.push((ws, BackendHandle { capabilities, tx, rx: unused }));
    }
    tokio::spawn(async move {
        while let Some(request) = end.rx.recv().await {
            let to = workspace_of(&request).and_then(|ws| senders.iter().find(|(w, _)| w == ws));
            let refused = match to {
                Some((_, b)) => b.send(request).err(),
                None => Some(Envelope::failed(request.id, FailKind::Auth, "no workspace signed in for this request")),
            };
            if let Some(failed) = refused
                && end.tx.send(failed).await.is_err()
            {
                return;
            }
        }
    });
    handle
}
