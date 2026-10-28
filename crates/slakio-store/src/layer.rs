//! The cache in front of a backend: a task between the UI's handle and the backend's, so the
//! UI and the backend each see the protocol they know.
//!
//! - A boot is answered first from the cache (`CachedBoot`, the last boot of this workspace)
//!   when it has one, then handed to the backend: the cache's answer is queued for the UI
//!   before the request is handed on, so a start draws from it without waiting for Slack. The
//!   cache is waited for [`BOOT_WAIT`] at most (a page [`READ_WAIT`]): a cache that is busy or
//!   slow never holds a request back, which then goes on without a cached answer, and the error
//!   log says so. The backend's `Booted` is kept and handed to the UI.
//! - The newest page of a conversation or a thread of this workspace likewise
//!   (`CachedHistory`); every page the backend answers for one is kept.
//! - Everything else goes through untouched, both ways: the UI's request still gets exactly one
//!   answer of the backend's (a cached one is extra and ends nothing).
//!
//! Every conversation asked for during the run, by any workspace's front, is kept from pruning
//! (a pane may show it); one cache is shared by every workspace's front. The cache is pruned after the first boot the backend answers, and after every
//! [`PRUNE_EVERY`] pages.

use crate::cache::Cache;
use slakio_core::backend::{BackendEnd, BackendHandle, Command, Envelope, Event, Generation, Request};
use slakio_core::fault::Fault;
use slakio_core::model::{Ts, WorkspaceId};
use std::collections::HashMap;
use std::time::Duration;

/// How many requests, and how many events, wait in the handle's channels at most.
pub const BOUND: usize = 64;

/// The longest a boot waits for the cache's snapshot before it is handed on without one (a large
/// workspace's snapshot takes longer to read than a page).
pub const BOOT_WAIT: Duration = Duration::from_millis(500);

/// The longest a request for a newest page waits for the cache's answer.
pub const READ_WAIT: Duration = Duration::from_millis(100);

/// How long requests wait for the cache's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Waits {
    pub boot: Duration,
    pub page: Duration,
}

impl Default for Waits {
    fn default() -> Self {
        Self { boot: BOOT_WAIT, page: READ_WAIT }
    }
}

/// Where a cache that did not answer in time is said (the error log).
pub type OnSlow = Box<dyn Fn(&Fault) + Send + Sync>;

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

/// Put `cache` in front of `inner`, the backend of `workspace`, waiting for it `waits` at most:
/// the handle the UI uses.
pub fn serve(
    inner: BackendHandle,
    cache: Cache,
    workspace: WorkspaceId,
    waits: Waits,
    on_slow: OnSlow,
) -> BackendHandle {
    let (handle, end) = BackendHandle::pair(inner.capabilities, BOUND);
    tokio::spawn(run(end, inner, Front { cache, workspace, waits, on_slow }));
    handle
}

/// The cache, and what it answers for.
struct Front {
    cache: Cache,
    workspace: WorkspaceId,
    waits: Waits,
    on_slow: OnSlow,
}

async fn run(mut end: BackendEnd, mut inner: BackendHandle, front: Front) {
    let cache = &front.cache;
    let mut asked: HashMap<Generation, Asked> = HashMap::new();
    let (mut pages, mut booted) = (0u32, false);
    loop {
        tokio::select! {
            request = end.rx.recv() => {
                let Some(request) = request else { return };
                if let Some(cached) = ask(&request, &front, &mut asked).await
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
                        // Of this workspace only: an answer never writes another's part.
                        cache.put_snapshot(s.only(&front.workspace));
                        if !booted {
                            booted = true;
                            cache.prune();
                        }
                    }
                    (Some(Asked::Page { before }), Event::History(p)) => {
                        cache.put_page((**p).clone(), before);
                        pages += 1;
                        if pages % PRUNE_EVERY == 0 {
                            cache.prune();
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
async fn ask(request: &Request, front: &Front, asked: &mut HashMap<Generation, Asked>) -> Option<Envelope> {
    let id = request.id;
    match &request.command {
        Command::Boot => {
            asked.insert(id, Asked::Boot);
            let read = front.cache.snapshot(vec![front.workspace.clone()]);
            let s = within(front.waits.boot, read, "the boot snapshot", &front.on_slow).await?;
            Some(Envelope { generation: id, event: Event::CachedBoot(Box::new(s)) })
        }
        Command::History { target, before, limit } if *target.workspace() == front.workspace => {
            front.cache.protect(target.clone());
            asked.insert(id, Asked::Page { before: *before });
            if before.is_some() {
                return None;
            }
            let read = front.cache.newest(target.clone(), *limit);
            let page = within(front.waits.page, read, "a newest page", &front.on_slow).await?;
            Some(Envelope { generation: id, event: Event::CachedHistory(Box::new(page)) })
        }
        _ => None,
    }
}

/// What `read` answers within `wait`; nothing when it takes longer, and `on_slow` is told.
async fn within<T>(wait: Duration, read: impl Future<Output = Option<T>>, what: &str, on_slow: &OnSlow) -> Option<T> {
    match tokio::time::timeout(wait, read).await {
        Ok(got) => got,
        Err(_) => {
            on_slow(&Fault::other(format!(
                "cache: {what} took longer than {} ms; went on without it",
                wait.as_millis()
            )));
            None
        }
    }
}
