//! The local cache as the binary sets it up (`slakio-store`): opened once, when the first
//! workspace's backend starts, and put in front of every signed-in workspace's backend (one
//! cache, a part per workspace); kept out of Time Machine backups on macOS; removed with
//! `slakio logout` (one workspace's part when signing out of one of several). Each workspace's
//! part is the user signed in to it's only: another sign-in finds that part emptied. Whatever
//! goes wrong with it, the app runs without it and says so once (the notices are queued, so
//! each is seen); the detail goes to the error log. The demo never uses it.

use slakio_core::backend::BackendHandle;
use slakio_core::config::Config;
use slakio_core::fault::{ErrorLog, Fault};
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::WorkspaceId;
use slakio_core::paths::Paths;
use slakio_store::cache::system_clock;
use slakio_store::layer::Waits;
use slakio_store::{Cache, Caps, OpenError, Store, Tmutil};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// `SLAKIO_TEST_TMUTIL`, debug builds only: a stand-in for `tmutil` the tests record calls with.
/// Without it a debug build excludes nothing, so no development run or test touches Time Machine.
const TEST_TMUTIL: &str = "SLAKIO_TEST_TMUTIL";

/// `SLAKIO_TEST_CACHE_WAIT_MS`, debug builds only: how long a request waits for the cache's
/// answer, so the tests' warm starts draw from the cache on a slow machine too.
const TEST_CACHE_WAIT: &str = "SLAKIO_TEST_CACHE_WAIT_MS";

/// `SLAKIO_TEST_CLAIM_DELAY_MS`, debug builds only: claiming a workspace's part of the cache
/// takes this long first, as with a slow disk, for the test that it never holds up the screen.
const TEST_CLAIM_DELAY: &str = "SLAKIO_TEST_CLAIM_DELAY_MS";

/// The one-time notice of a failed exclusion from backups.
const BACKUP_NOTICE: &str = "cache_backup_failed";

/// How long making a workspace's part its user's may take before it goes on without the cache.
const CLAIM_WAIT: Duration = Duration::from_secs(2);

/// What opening the cache needs, and the cache once open.
#[derive(Clone)]
pub(crate) struct Local {
    on: bool,
    caps: Caps,
    paths: Paths,
    /// Opened (or tried) once for the process: a second open of the same process would find its
    /// own lock and read only.
    opened: Arc<Mutex<Opening>>,
}

#[derive(Default)]
struct Opening {
    tried: bool,
    cache: Option<Cache>,
}

/// Add `msg` to `notices` unless it is there already (several workspaces, one notice).
fn notice(notices: &mut Vec<Msg>, msg: Msg) {
    if !notices.contains(&msg) {
        notices.push(msg);
    }
}

/// The error log, written off the caller's thread: a slow disk never holds up the task that
/// noticed the cache was slow.
fn log_off_thread(errors: Option<PathBuf>) -> slakio_store::layer::OnSlow {
    Box::new(move |f: &Fault| {
        let (errors, f) = (errors.clone(), f.clone());
        let write = move || ErrorLog::new(errors).record("cache", &f);
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => drop(rt.spawn_blocking(write)),
            Err(_) => write(),
        }
    })
}

impl Local {
    pub(crate) fn new(paths: &Paths, cfg: &Config) -> Self {
        let caps = Caps {
            per_conversation: cfg.cache_messages,
            max_bytes: u64::from(cfg.cache_max_mb) * 1024 * 1024,
            max_days: cfg.cache_days,
        };
        Self { on: cfg.cache == "on", caps, paths: paths.clone(), opened: Arc::default() }
    }

    fn record(&self, fault: &Fault) {
        ErrorLog::new(self.paths.errors_log()).record("cache", fault);
    }

    /// `backend`, the backend of workspace `team` signed in as `user`, with the cache in front of
    /// it when it can be had; what to tell the user about it goes to `notices` (once each).
    pub(crate) fn front(
        &self,
        backend: BackendHandle,
        team: &str,
        user: &str,
        notices: &mut Vec<Msg>,
    ) -> BackendHandle {
        let Some(cache) = self.cache(notices) else { return backend };
        if let Some(ms) = cfg!(debug_assertions).then(|| std::env::var(TEST_CLAIM_DELAY).ok()?.parse().ok()).flatten() {
            std::thread::sleep(Duration::from_millis(ms));
        }
        // Another sign-in's part: emptied by the writer, not used by a reader.
        match cache.claim(team, user, CLAIM_WAIT) {
            Some(Ok(true)) => {
                let log = log_off_thread(self.paths.errors_log());
                slakio_store::layer::serve(backend, cache, WorkspaceId::new(team), waits(), log)
            }
            Some(Ok(false)) => {
                notice(notices, Msg::Label(Label::CacheInUse));
                backend
            }
            Some(Err(fault)) => {
                self.record(&fault);
                notice(notices, Msg::Label(Label::CacheFailed));
                backend
            }
            None => {
                self.record(&Fault::other("cache: a workspace's part could not be claimed in time"));
                notice(notices, Msg::Label(Label::CacheFailed));
                backend
            }
        }
    }

    /// The cache, opened the first time it is asked for (what opening it found goes to
    /// `notices`); `None` when it is off or could not be had.
    fn cache(&self, notices: &mut Vec<Msg>) -> Option<Cache> {
        let mut o = self.opened.lock().unwrap_or_else(PoisonError::into_inner);
        if !std::mem::replace(&mut o.tried, true) {
            o.cache = self.open(notices);
        }
        o.cache.clone()
    }

    fn open(&self, notices: &mut Vec<Msg>) -> Option<Cache> {
        let dir = self.paths.cache.as_ref().filter(|_| self.on)?;
        let opened = match Store::open(dir) {
            Ok(o) => o,
            Err(OpenError::InUse) => {
                notice(notices, Msg::Label(Label::CacheInUse));
                return None;
            }
            Err(OpenError::Failed(fault)) => {
                self.record(&fault);
                notice(notices, Msg::Label(Label::CacheFailed));
                return None;
            }
        };
        if opened.rebuilt {
            self.record(&Fault::other("cache: unreadable or of a newer version; set up again"));
            notice(notices, Msg::Label(Label::CacheRebuilt));
        }
        if opened.store.read_only() {
            notice(notices, Msg::Label(Label::CacheReadOnly));
        } else if let Some(tmutil) = exclusion() {
            // Once: a cache that was excluded remembers it.
            if let Err(why) = opened.store.exclude_from_backups(&tmutil) {
                self.record(&Fault::other(format!("cache: not excluded from backups: {why}")));
                if self.paths.first_time(BACKUP_NOTICE) {
                    notices.push(Msg::Label(Label::CacheBackupFailed));
                }
            }
        }
        let errors = self.paths.errors_log();
        let log = Box::new(move |f: &Fault| ErrorLog::new(errors.clone()).record("cache", f));
        match Cache::start(opened.store, self.caps, system_clock(), log) {
            Ok(cache) => Some(cache),
            Err(e) => {
                self.record(&Fault::io(&e));
                notice(notices, Msg::Label(Label::CacheFailed));
                None
            }
        }
    }

    /// Before the process ends: what the cache was asked to keep is written (two seconds at most).
    pub(crate) fn flush(&self) {
        if let Some(cache) = self.opened.lock().unwrap_or_else(PoisonError::into_inner).cache.take() {
            cache.flush(Duration::from_secs(2));
        }
    }
}

/// How long requests wait for the cache: the store's own bounds; in a debug build the tests'
/// [`TEST_CACHE_WAIT`] when it is set.
fn waits() -> Waits {
    let test = cfg!(debug_assertions).then(|| std::env::var(TEST_CACHE_WAIT).ok()?.parse().ok()).flatten();
    test.map_or_else(Waits::default, |ms| Waits { boot: Duration::from_millis(ms), page: Duration::from_millis(ms) })
}

/// How the cache directory is kept out of backups: `tmutil` on macOS; in a debug build only the
/// tests' stand-in, when one is named.
fn exclusion() -> Option<Tmutil> {
    if cfg!(debug_assertions) {
        return std::env::var_os(TEST_TMUTIL).map(|p| Tmutil { program: PathBuf::from(p) });
    }
    cfg!(target_os = "macos").then(Tmutil::default)
}

/// `slakio logout`: nothing of the cache is left.
pub(crate) fn remove(paths: &Paths) -> Result<(), Fault> {
    match &paths.cache {
        Some(dir) if dir.exists() => slakio_store::db::remove(dir),
        _ => Ok(()),
    }
}

/// `slakio logout <workspace>` while other workspaces stay signed in: the cache, held as its
/// writer from before the workspace is signed out until its part is removed, so no slakio starts
/// writing that part meanwhile. `Ok(None)`: there is no cache. `Err(None)`: a slakio runs and
/// writes the cache (it is to be quit first).
pub(crate) fn hold(paths: &Paths) -> Result<Option<Store>, Option<Fault>> {
    let Some(dir) = paths.cache.as_ref().filter(|d| d.join(slakio_store::db::DIR).exists()) else { return Ok(None) };
    let opened = match Store::open(dir) {
        Ok(o) => o,
        Err(OpenError::InUse) => return Err(None),
        Err(OpenError::Failed(f)) => return Err(Some(f)),
    };
    if opened.store.read_only() {
        return Err(None);
    }
    Ok(Some(opened.store))
}

/// A slakio runs and uses the cache (it holds the writer's lock).
pub(crate) fn in_use(paths: &Paths) -> bool {
    paths.cache.as_deref().is_some_and(slakio_store::db::in_use)
}
