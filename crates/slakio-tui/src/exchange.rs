//! Between the app and its backend: hand the app's requests to the backend and the backend's
//! answers to the app. An answer can ask for more (a page that arrives while `gg` waits for the
//! oldest message asks for the next one), so one round is not enough: the requests an answer
//! queued are sent in the same turn, not when the next key happens to wake the loop up.
//!
//! [`exchange`] drives a backend that answers in turn until neither side has more (the tests).
//! The binary runs the backend on its own task behind a [`BackendHandle`] ([`serve`]): [`pump`]
//! sends and takes what is ready, and an answer that comes later wakes the loop ([`next`]).

use crate::app::App;
use slakio_core::backend::{Backend, BackendHandle, Envelope};
use std::time::Instant;

/// How many requests, and how many events, wait in a handle's channels at most.
pub const BOUND: usize = 64;

/// One turn of the exchange with a backend that answers in turn. `true` when an answer changed
/// the screen.
pub fn exchange(app: &mut App, backend: &mut dyn Backend, now: Instant) -> bool {
    let mut changed = false;
    loop {
        let requests = app.take_commands();
        let sent = !requests.is_empty();
        for request in requests {
            backend.send(request);
        }
        let mut answered = false;
        while let Some(envelope) = backend.poll() {
            answered = true;
            changed |= app.on_backend(envelope, now);
        }
        if !sent && !answered {
            return changed;
        }
    }
}

/// Run `backend` on a task of its own behind a handle (inside a Tokio runtime).
pub fn serve(mut backend: Box<dyn Backend + Send>) -> BackendHandle {
    let (handle, mut end) = BackendHandle::pair(backend.capabilities(), BOUND);
    tokio::spawn(async move {
        while let Some(request) = end.rx.recv().await {
            backend.send(request);
            while let Some(envelope) = backend.poll() {
                if end.tx.send(envelope).await.is_err() {
                    return;
                }
            }
        }
    });
    handle
}

/// One turn of the exchange over `handle`: send what the app asked for and take what is ready.
/// After sending it lets the backend's task run once, so a backend that answers at once (the
/// demo) is drawn in the same turn; whatever comes later arrives through [`next`]. `true` when
/// an answer changed the screen.
pub async fn pump(app: &mut App, handle: &mut BackendHandle) -> bool {
    let mut changed = false;
    loop {
        let requests = app.take_commands();
        let sent = !requests.is_empty();
        for request in requests {
            if let Err(failed) = handle.send(request) {
                changed |= app.on_backend(failed, Instant::now());
            }
        }
        if sent {
            tokio::task::yield_now().await;
        }
        let mut answered = false;
        while let Ok(envelope) = handle.rx.try_recv() {
            answered = true;
            changed |= app.on_backend(envelope, Instant::now());
        }
        if !sent && !answered {
            return changed;
        }
    }
}

/// The next event of the backend, whenever it comes; never, without a backend (or once it is
/// gone).
pub async fn next(handle: Option<&mut BackendHandle>) -> Envelope {
    match handle {
        Some(h) => match h.rx.recv().await {
            Some(envelope) => envelope,
            None => std::future::pending().await,
        },
        None => std::future::pending().await,
    }
}
