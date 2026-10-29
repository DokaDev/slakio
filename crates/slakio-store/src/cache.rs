//! The cache on a thread of its own: the store is used by that thread only, one job after
//! another in the order asked, so a write never races another and the UI's loop never waits on
//! the disk. Reads answer through a channel the caller awaits; writes are not waited for.
//!
//! A prune gives the pages it freed back to the file system a few at a time, the jobs asked for
//! meanwhile going first, so a read waits behind it little.
//!
//! A job that fails goes to `on_fault` (the error log), and the cache carries on: a read that
//! fails is "nothing cached", never a wrong answer. A database found damaged while in use is set
//! up again from nothing.

use crate::db::{Store, fault, is_corrupt};
use crate::prune::{Caps, Pruned};
use slakio_core::backend::{Page, Snapshot};
use slakio_core::fault::Fault;
use slakio_core::model::{Target, Ts, WorkspaceId};
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::{mpsc, oneshot};

/// The most freed pages one step gives back to the file system.
const VACUUM_PAGES: u32 = 256;

/// Where a failed job's fault goes.
pub type OnFault = Box<dyn Fn(&Fault) + Send>;
/// The time now, in seconds since the Unix epoch.
pub type Clock = Box<dyn Fn() -> u64 + Send>;

/// The system's clock.
pub fn system_clock() -> Clock {
    Box::new(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()))
}

enum Job {
    Snapshot {
        expected: Vec<WorkspaceId>,
        reply: oneshot::Sender<Option<Snapshot>>,
    },
    Newest {
        target: Target,
        limit: u32,
        reply: oneshot::Sender<Option<Page>>,
    },
    PutSnapshot(Box<Snapshot>, Writer),
    PutPage {
        page: Box<Page>,
        before: Option<Ts>,
        writer: Writer,
    },
    Prune {
        protect: HashSet<Target>,
        reply: Option<oneshot::Sender<Pruned>>,
    },
    /// The next step of giving freed pages back.
    Vacuum,
    Claim {
        team: String,
        user: String,
        reply: std::sync::mpsc::Sender<Result<bool, Fault>>,
    },
    Flush(std::sync::mpsc::Sender<()>),
}

/// Who a write is from: the sign-in (team, user) whose part it is for, when it is one's
/// ([`Cache::writing_as`]); the writer drops it once that part is no longer that sign-in's.
type Writer = Option<Arc<(String, String)>>;

/// The cache's thread, as its users see it. Cloned freely; the thread ends with the last clone.
/// Every workspace's backend shares one: the conversations any of them asked for are kept from
/// pruning together.
#[derive(Clone, Debug)]
pub struct Cache {
    tx: mpsc::UnboundedSender<Job>,
    read_only: bool,
    protect: Arc<Mutex<HashSet<Target>>>,
    writer: Writer,
}

impl Cache {
    /// Run `store` on a thread of its own, keeping it within `caps` by `clock`.
    pub fn start(store: Store, caps: Caps, clock: Clock, on_fault: OnFault) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let read_only = store.read_only();
        std::thread::Builder::new()
            .name("slakio-cache".into())
            .spawn(move || run(store, caps, &*clock, &*on_fault, rx))?;
        Ok(Self { tx, read_only, protect: Arc::default(), writer: None })
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

    /// This cache, its writes made for the sign-in of `user` to workspace `team` (claimed for it
    /// first): once another sign-in claims that part, or it is forgotten, they are dropped, so a
    /// sign-in replaced never writes into the part of the one after it.
    #[must_use]
    pub fn writing_as(&self, team: &str, user: &str) -> Self {
        Self { writer: Some(Arc::new((team.to_owned(), user.to_owned()))), ..self.clone() }
    }

    /// Keep `snapshot` as the last boot's.
    pub fn put_snapshot(&self, snapshot: Snapshot) {
        let _ = self.tx.send(Job::PutSnapshot(Box::new(snapshot), self.writer.clone()));
    }

    /// Keep `page`, asked for as the messages older than `before`.
    pub fn put_page(&self, page: Page, before: Option<Ts>) {
        let _ = self.tx.send(Job::PutPage { page: Box::new(page), before, writer: self.writer.clone() });
    }

    /// Keep `target` from pruning for the life of the process (a pane may show it).
    pub fn protect(&self, target: Target) {
        self.protect.lock().unwrap_or_else(PoisonError::into_inner).insert(target);
    }

    fn protected(&self) -> HashSet<Target> {
        self.protect.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Prune to the caps, never touching a conversation kept from it ([`Cache::protect`]), of
    /// whichever workspace.
    pub fn prune(&self) {
        let _ = self.tx.send(Job::Prune { protect: self.protected(), reply: None });
    }

    /// [`Cache::prune`], and what it removed.
    pub async fn pruned(&self) -> Option<Pruned> {
        let (reply, answer) = oneshot::channel();
        self.tx.send(Job::Prune { protect: self.protected(), reply: Some(reply) }).ok()?;
        answer.await.ok()
    }

    /// Make workspace `team`'s part `user`'s ([`Store::claim`]), waiting at most `wait` (`None`
    /// when it took longer, or the thread is gone).
    pub fn claim(&self, team: &str, user: &str, wait: std::time::Duration) -> Option<Result<bool, Fault>> {
        let (reply, answer) = std::sync::mpsc::channel();
        let job = Job::Claim { team: team.to_owned(), user: user.to_owned(), reply };
        self.tx.send(job).ok()?;
        answer.recv_timeout(wait).ok()
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
    // Jobs taken from the channel ahead of a vacuum step, in the order asked.
    let mut later: VecDeque<Job> = VecDeque::new();
    loop {
        let Some(job) = later.pop_front().or_else(|| rx.blocking_recv()) else { return };
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
            // A replaced sign-in's late answer: its part is another's now.
            Job::PutSnapshot(_, Some(w)) | Job::PutPage { writer: Some(w), .. } if !store.owned_by(&w.0, &w.1) => {}
            Job::PutSnapshot(s, _) => {
                if let Err(e) = store.put_snapshot(&s) {
                    failed(&mut store, &e);
                }
            }
            Job::PutPage { page, before, .. } => {
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
                later.push_back(Job::Vacuum);
            }
            Job::Vacuum => match store.vacuum(VACUUM_PAGES) {
                Ok(true) => {
                    // More to give back: what was asked for meanwhile goes first.
                    while let Ok(job) = rx.try_recv() {
                        later.push_back(job);
                    }
                    later.push_back(Job::Vacuum);
                }
                Ok(false) => {}
                Err(e) => failed(&mut store, &e),
            },
            Job::Claim { team, user, reply } => {
                let _ = reply.send(store.claim(&team, &user));
            }
            Job::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}
