//! The cache on a thread of its own: the store is used by that thread only, one job after
//! another in the order asked, so a write never races another and the UI's loop never waits on
//! the disk. Reads answer through a channel the caller awaits; writes are not waited for.
//!
//! A job that fails goes to `on_fault` (the error log), and the cache carries on: a read that
//! fails is "nothing cached", never a wrong answer. A database found damaged while in use is set
//! up again from nothing.

use crate::db::{Store, fault, is_corrupt};
use crate::prune::{Caps, Pruned};
use slakio_core::backend::{Page, Snapshot};
use slakio_core::fault::Fault;
use slakio_core::model::{Target, Ts, WorkspaceId};
use std::collections::HashSet;
use tokio::sync::{mpsc, oneshot};

/// Where a failed job's fault goes.
pub type OnFault = Box<dyn Fn(&Fault) + Send>;
/// The time now, in seconds since the Unix epoch.
pub type Clock = Box<dyn Fn() -> u64 + Send>;

/// The system's clock.
pub fn system_clock() -> Clock {
    Box::new(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()))
}

enum Job {
    Snapshot { expected: Vec<WorkspaceId>, reply: oneshot::Sender<Option<Snapshot>> },
    Newest { target: Target, limit: u32, reply: oneshot::Sender<Option<Page>> },
    PutSnapshot(Box<Snapshot>),
    PutPage { page: Box<Page>, before: Option<Ts> },
    Prune { protect: HashSet<Target>, reply: Option<oneshot::Sender<Pruned>> },
    Flush(std::sync::mpsc::Sender<()>),
}

/// The cache's thread, as its users see it. Cloned freely; the thread ends with the last clone.
#[derive(Clone, Debug)]
pub struct Cache {
    tx: mpsc::UnboundedSender<Job>,
    read_only: bool,
}

impl Cache {
    /// Run `store` on a thread of its own, keeping it within `caps` by `clock`.
    pub fn start(store: Store, caps: Caps, clock: Clock, on_fault: OnFault) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let read_only = store.read_only();
        std::thread::Builder::new()
            .name("slakio-cache".into())
            .spawn(move || run(store, caps, &*clock, &*on_fault, rx))?;
        Ok(Self { tx, read_only })
    }

    /// Another process writes the cache: this one only reads it.
    pub fn read_only(&self) -> bool {
        self.read_only
    }

    /// The last boot's snapshot when it lists exactly `expected`.
    pub async fn snapshot(&self, expected: Vec<WorkspaceId>) -> Option<Snapshot> {
        let (reply, answer) = oneshot::channel();
        self.tx.send(Job::Snapshot { expected, reply }).ok()?;
        answer.await.ok().flatten()
    }

    /// The newest `limit` messages kept for `target`.
    pub async fn newest(&self, target: Target, limit: u32) -> Option<Page> {
        let (reply, answer) = oneshot::channel();
        self.tx.send(Job::Newest { target, limit, reply }).ok()?;
        answer.await.ok().flatten()
    }

    /// Keep `snapshot` as the last boot's.
    pub fn put_snapshot(&self, snapshot: Snapshot) {
        let _ = self.tx.send(Job::PutSnapshot(Box::new(snapshot)));
    }

    /// Keep `page`, asked for as the messages older than `before`.
    pub fn put_page(&self, page: Page, before: Option<Ts>) {
        let _ = self.tx.send(Job::PutPage { page: Box::new(page), before });
    }

    /// Prune to the caps, never touching `protect`.
    pub fn prune(&self, protect: HashSet<Target>) {
        let _ = self.tx.send(Job::Prune { protect, reply: None });
    }

    /// [`Cache::prune`], and what it removed.
    pub async fn pruned(&self, protect: HashSet<Target>) -> Option<Pruned> {
        let (reply, answer) = oneshot::channel();
        self.tx.send(Job::Prune { protect, reply: Some(reply) }).ok()?;
        answer.await.ok()
    }

    /// Wait (at most `wait`) until every job asked for so far is done: before the process ends,
    /// so the last page is kept. `false` when it took longer, or the thread is gone.
    pub fn flush(&self, wait: std::time::Duration) -> bool {
        let (done, finished) = std::sync::mpsc::channel();
        self.tx.send(Job::Flush(done)).is_ok() && finished.recv_timeout(wait).is_ok()
    }
}

fn run(
    mut store: Store,
    caps: Caps,
    clock: &dyn Fn() -> u64,
    on_fault: &dyn Fn(&Fault),
    mut rx: mpsc::UnboundedReceiver<Job>,
) {
    // A failure: logged; a damaged database is set up again (what it held is lost, and fetched
    // again).
    let failed = |store: &mut Store, e: &rusqlite::Error| {
        on_fault(&fault(e));
        if is_corrupt(e)
            && let Err(f) = store.rebuild()
        {
            on_fault(&f);
        }
    };
    while let Some(job) = rx.blocking_recv() {
        match job {
            Job::Snapshot { expected, reply } => {
                let got = store.snapshot(&expected).unwrap_or_else(|e| {
                    failed(&mut store, &e);
                    None
                });
                let _ = reply.send(got);
            }
            Job::Newest { target, limit, reply } => {
                let got = store.newest_page(&target, limit).unwrap_or_else(|e| {
                    failed(&mut store, &e);
                    None
                });
                let _ = reply.send(got);
            }
            Job::PutSnapshot(s) => {
                if let Err(e) = store.put_snapshot(&s) {
                    failed(&mut store, &e);
                }
            }
            Job::PutPage { page, before } => {
                if let Err(e) = store.put_page(&page, before, clock()) {
                    failed(&mut store, &e);
                }
            }
            Job::Prune { protect, reply } => {
                let pruned = store.prune(caps, &protect, clock()).unwrap_or_else(|e| {
                    failed(&mut store, &e);
                    Pruned::default()
                });
                if let Some(r) = reply {
                    let _ = r.send(pruned);
                }
            }
            Job::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}
