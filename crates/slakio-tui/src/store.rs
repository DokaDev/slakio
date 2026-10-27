//! The local cache as the binary sets it up (`slakio-store`): opened for the signed-in workspace
//! when its backend starts, put in front of it, kept out of Time Machine backups on macOS, and
//! removed with `slakio logout`. Whatever goes wrong with it, the app runs without it and says so
//! once; the detail goes to the error log. The demo never uses it.

use slakio_core::backend::BackendHandle;
use slakio_core::config::Config;
use slakio_core::fault::{ErrorLog, Fault};
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::WorkspaceId;
use slakio_core::paths::Paths;
use slakio_store::cache::system_clock;
use slakio_store::{Cache, Caps, OpenError, Store, Tmutil};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// `SLAKIO_TEST_TMUTIL`, debug builds only: a stand-in for `tmutil` the tests record calls with.
/// Without it a debug build excludes nothing, so no development run or test touches Time Machine.
const TEST_TMUTIL: &str = "SLAKIO_TEST_TMUTIL";

/// The one-time notice of a failed exclusion from backups.
const BACKUP_NOTICE: &str = "cache_backup_failed";

/// What opening the cache needs, and the cache once open.
#[derive(Clone)]
pub(crate) struct Local {
    on: bool,
    /// Requests may be sent: a start without network fails its boot with a warning that takes
    /// the status line's one slot, so a one-time notice is not counted as shown then.
    network: bool,
    caps: Caps,
    paths: Paths,
    opened: Arc<Mutex<Option<Cache>>>,
}

impl Local {
    pub(crate) fn new(paths: &Paths, cfg: &Config, network: bool) -> Self {
        let caps = Caps {
            per_conversation: cfg.cache_messages,
            max_bytes: u64::from(cfg.cache_max_mb) * 1024 * 1024,
            max_days: cfg.cache_days,
        };
        Self { on: cfg.cache == "on", network, caps, paths: paths.clone(), opened: Arc::default() }
    }

    fn record(&self, fault: &Fault) {
        ErrorLog::new(self.paths.errors_log()).record("cache", fault);
    }

    /// `backend`, the backend of workspace `team`, with the cache in front of it when it can be
    /// had; what to tell the user about it goes to `notices`.
    pub(crate) fn front(&self, backend: BackendHandle, team: &str, notices: &mut Vec<Msg>) -> BackendHandle {
        match self.open(notices) {
            Some(cache) => slakio_store::layer::serve(backend, cache, WorkspaceId::new(team)),
            None => backend,
        }
    }

    fn open(&self, notices: &mut Vec<Msg>) -> Option<Cache> {
        let dir = self.paths.cache.as_ref().filter(|_| self.on)?;
        let opened = match Store::open(dir) {
            Ok(o) => o,
            Err(OpenError::InUse) => {
                notices.push(Msg::Label(Label::CacheInUse));
                return None;
            }
            Err(OpenError::Failed(fault)) => {
                self.record(&fault);
                notices.push(Msg::Label(Label::CacheFailed));
                return None;
            }
        };
        if opened.rebuilt {
            self.record(&Fault::other("cache: unreadable or of a newer version; set up again"));
            notices.push(Msg::Label(Label::CacheRebuilt));
        }
        if opened.store.read_only() {
            notices.push(Msg::Label(Label::CacheReadOnly));
        } else if let Some(tmutil) = exclusion() {
            // Once: a cache that was excluded remembers it.
            if let Err(why) = opened.store.exclude_from_backups(&tmutil) {
                self.record(&Fault::other(format!("cache: not excluded from backups: {why}")));
                if !self.network || self.paths.first_time(BACKUP_NOTICE) {
                    notices.push(Msg::Label(Label::CacheBackupFailed));
                }
            }
        }
        let errors = self.paths.errors_log();
        let log = Box::new(move |f: &Fault| ErrorLog::new(errors.clone()).record("cache", f));
        match Cache::start(opened.store, self.caps, system_clock(), log) {
            Ok(cache) => {
                *self.opened.lock().unwrap_or_else(PoisonError::into_inner) = Some(cache.clone());
                Some(cache)
            }
            Err(e) => {
                self.record(&Fault::io(&e));
                notices.push(Msg::Label(Label::CacheFailed));
                None
            }
        }
    }

    /// Before the process ends: what the cache was asked to keep is written (two seconds at most).
    pub(crate) fn flush(&self) {
        if let Some(cache) = self.opened.lock().unwrap_or_else(PoisonError::into_inner).take() {
            cache.flush(Duration::from_secs(2));
        }
    }
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
