//! The cache in front of a backend: a task between the UI's handle and the backend's, so the
//! UI and the backend each see the protocol they know.
//!
//! - A boot is answered first from the cache (`CachedBoot`, the last boot of this workspace)
//!   when it has one, then handed to the backend: the cache's answer is queued for the UI
//!   before the request is handed on, so a start draws from it without waiting for Slack. The
//!   cache is waited for [`READ_WAIT`] at most: a cache that is busy or slow never holds a
//!   request back, which then goes on without a cached answer. The backend's `Booted` is kept
//!   and handed to the UI.
//! - The newest page of a conversation of this workspace likewise (`CachedHistory`); every page
//!   the backend answers for one is kept.
//! - Everything else goes through untouched, both ways: the UI's request still gets exactly one
//!   answer of the backend's (a cached one is extra and ends nothing).
//!
//! Every conversation asked for during the run is kept from pruning (a pane may show it). The
//! cache is pruned after the first boot the backend answers, and after every
//! [`PRUNE_EVERY`] pages.

use crate::cache::Cache;
use slakio_core::backend::{BackendEnd, BackendHandle, Command, Envelope, Event, Generation, Request};
use slakio_core::model::{Target, Ts, WorkspaceId};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// How many requests, and how many events, wait in the handle's channels at most.
pub const BOUND: usize = 64;

/// The longest a request waits for the cache's answer before it is handed on without one.
pub const READ_WAIT: Duration = Duration::from_millis(100);

/// The cache is pruned after this many pages kept.
pub const PRUNE_EVERY: u32 = 32;

/// The most answers waited for that are remembered (a backend may drop a superseded request
/// unanswered; older entries are forgotten).
const ASKED_MAX: u64 = 1024;

/// What a request handed on asked for, to keep its answer.
enum Asked {
    Boot,
    Page { before: Option<Ts> },
}

/// Put `cache` in front of `inner`, the backend of `workspace`: the handle the UI uses.
pub fn serve(inner: BackendHandle, cache: Cache, workspace: WorkspaceId) -> BackendHandle {
    let (handle, end) = BackendHandle::pair(inner.capabilities, BOUND);
    tokio::spawn(run(end, inner, cache, workspace));
    handle
}

async fn run(mut end: BackendEnd, mut inner: BackendHandle, cache: Cache, workspace: WorkspaceId) {
    let mut asked: HashMap<Generation, Asked> = HashMap::new();
    let mut protect: HashSet<Target> = HashSet::new();
    let (mut pages, mut booted) = (0u32, false);
    loop {
        tokio::select! {
            request = end.rx.recv() => {
                let Some(request) = request else { return };
                if let Some(cached) = ask(&request, &cache, &workspace, &mut protect, &mut asked).await
                    && end.tx.send(cached).await.is_err()
                {
                    return;
                }
                asked.retain(|g, _| g.0 + ASKED_MAX > request.id.0);
                if let Err(failed) = inner.send(request) {
                    asked.remove(&failed.generation);
                    if end.tx.send(failed).await.is_err() {
                        return;
                    }
                }
            }
            envelope = inner.rx.recv() => {
                let Some(envelope) = envelope else { return };
                match (asked.remove(&envelope.generation), &envelope.event) {
                    (Some(Asked::Boot), Event::Booted(s)) => {
                        cache.put_snapshot((**s).clone());
                        if !booted {
                            booted = true;
                            cache.prune(protect.clone());
                        }
                    }
                    (Some(Asked::Page { before }), Event::History(p)) => {
                        cache.put_page((**p).clone(), before);
                        pages += 1;
                        if pages % PRUNE_EVERY == 0 {
                            cache.prune(protect.clone());
                        }
                    }
                    _ => {}
                }
                if end.tx.send(envelope).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// Note what `request` asks for, and the cache's answer to give first, if it has one.
async fn ask(
    request: &Request,
    cache: &Cache,
    workspace: &WorkspaceId,
    protect: &mut HashSet<Target>,
    asked: &mut HashMap<Generation, Asked>,
) -> Option<Envelope> {
    let id = request.id;
    match &request.command {
        Command::Boot => {
            asked.insert(id, Asked::Boot);
            let s = tokio::time::timeout(READ_WAIT, cache.snapshot(vec![workspace.clone()])).await.ok().flatten()?;
            Some(Envelope { generation: id, event: Event::CachedBoot(Box::new(s)) })
        }
        Command::History { target: target @ Target::Conversation { workspace: ws, .. }, before, limit }
            if ws == workspace =>
        {
            protect.insert(target.clone());
            asked.insert(id, Asked::Page { before: *before });
            if before.is_some() {
                return None;
            }
            let page = tokio::time::timeout(READ_WAIT, cache.newest(target.clone(), *limit)).await.ok().flatten()?;
            Some(Envelope { generation: id, event: Event::CachedHistory(Box::new(page)) })
        }
        _ => None,
    }
}
