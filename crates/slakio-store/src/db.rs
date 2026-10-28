//! Opening the cache: one SQLite file in a directory of its own ([`DIR`]) inside the cache
//! directory, private to the user (that directory `0700`, the database and its journal files
//! `0600`; SQLite gives its `-wal` and `-shm` files the database file's mode), with one process
//! writing it at a time. Only that directory, which the store makes, is ever made private (or
//! excluded from backups): never the cache directory the user named. A symbolic link in place
//! of that directory or of any of its files is refused (the files are opened without following
//! one, and made private through the open file).
//!
//! The writer holds an OS lock on `cache.lock` (`File::try_lock`: `flock` on Unix, `LockFileEx`
//! on Windows), released when the process ends however it ends. A second slakio while one runs
//! opens the database read-only: it draws from the cache but never writes it
//! ([`Store::read_only`]); when there is nothing it could read it does without
//! ([`OpenError::InUse`]). A lock let go a moment ago may still be held, for as long as a
//! process being started shares the open file (until it runs its program): it is tried again for
//! [`LOCK_WAIT`] before the cache is opened read-only.
//!
//! A database that cannot be read (not a database, damaged) or that a newer slakio wrote is set
//! up again from nothing ([`Opened::rebuilt`]): it only holds what can be fetched again. Any
//! other failure (a directory that cannot be made, a full disk) is a fault: the app runs without
//! the cache and says so, and the file is left as it is.

use crate::schema::{self, MIGRATIONS, Schema};
use rusqlite::{Connection, ErrorCode, OpenFlags};
use slakio_core::fault::Fault;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The directory of the cache's files, which the store makes inside the cache directory.
pub const DIR: &str = "slakio-store";
/// The database's file name in [`DIR`].
pub const FILE: &str = "cache.sqlite3";
/// The writer's lock.
const LOCK: &str = "cache.lock";
/// The key under which the cache keeps whose it is.
const OWNER: &str = "owner";
/// SQLite's files beside the database.
const SIDE: [&str; 2] = ["-wal", "-shm"];
/// How long a held writer's lock is tried again before the cache is opened read-only.
pub const LOCK_WAIT: Duration = Duration::from_millis(50);
/// The pause between two tries.
const LOCK_STEP: Duration = Duration::from_millis(5);

/// The cache, open.
pub struct Store {
    pub(crate) conn: Connection,
    dir: PathBuf,
    /// Held while this process writes the cache (`None`: read-only).
    lock: Option<File>,
    /// Whose it is, once claimed: kept again when it is set up again.
    owner: Option<String>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").field("dir", &self.dir).field("read_only", &self.read_only()).finish_non_exhaustive()
    }
}

/// The cache as [`Store::open`] found it.
#[derive(Debug)]
pub struct Opened {
    pub store: Store,
    /// What was there could not be read, or a newer version wrote it: it was set up again,
    /// empty.
    pub rebuilt: bool,
}

/// Why the cache could not be opened.
#[derive(Debug)]
pub enum OpenError {
    /// Another slakio writes the cache, and it holds nothing this one could read.
    InUse,
    /// It could not be opened (the detail is for the error log).
    Failed(Fault),
}

/// A database error for the error log (SQLite's words, never a value of a row).
pub(crate) fn fault(e: &rusqlite::Error) -> Fault {
    Fault::other(format!("cache: {e}"))
}

fn io_fault(e: &std::io::Error, path: &Path) -> Fault {
    Fault::io_at(e, path)
}

/// The database cannot be read as one (not a database, or damaged).
pub fn is_corrupt(e: &rusqlite::Error) -> bool {
    matches!(e.sqlite_error_code(), Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase))
}

impl Store {
    /// Open the cache in the cache directory `parent` (its own directory there made if missing).
    pub fn open(parent: &Path) -> Result<Opened, OpenError> {
        Self::open_waiting(parent, LOCK_WAIT)
    }

    /// [`Store::open`], trying a held lock again for `wait`.
    fn open_waiting(parent: &Path, wait: Duration) -> Result<Opened, OpenError> {
        let dir = &own_dir(parent).map_err(OpenError::Failed)?;
        let lock_path = dir.join(LOCK);
        let lock = private_file(&lock_path).map_err(|e| OpenError::Failed(io_fault(&e, &lock_path)))?;
        let path = dir.join(FILE);
        if !locked(&lock, wait).map_err(|e| OpenError::Failed(io_fault(&e, &lock_path)))? {
            let conn = open_read_only(&path).ok_or(OpenError::InUse)?;
            let store = Store { conn, dir: dir.to_path_buf(), lock: None, owner: None };
            return Ok(Opened { store, rebuilt: false });
        }
        let (conn, rebuilt) = match open_writer(&path) {
            Ok(conn) => (conn, false),
            Err(Unusable::Damaged) => {
                remove_files(dir).map_err(OpenError::Failed)?;
                (open_writer(&path).map_err(Unusable::into_fault).map_err(OpenError::Failed)?, true)
            }
            Err(Unusable::Fault(f)) => return Err(OpenError::Failed(f)),
        };
        Ok(Opened { store: Store { conn, dir: dir.to_path_buf(), lock: Some(lock), owner: None }, rebuilt })
    }

    /// Another process writes the cache: this one only reads it.
    pub fn read_only(&self) -> bool {
        self.lock.is_none()
    }

    /// Make the cache `owner`'s (the signed-in workspace and user): one that another sign-in
    /// kept, or that does not say whose it is, is emptied first. `Ok(false)`: it is not
    /// `owner`'s and this process only reads it, so it is not to be used.
    pub fn claim(&mut self, owner: &str) -> Result<bool, Fault> {
        if self.meta(OWNER)?.as_deref() == Some(owner) {
            self.owner = Some(owner.to_owned());
            return Ok(true);
        }
        if self.read_only() {
            return Ok(false);
        }
        // Only once emptied is it `owner`'s: a rebuild that failed leaves it the one it was.
        self.rebuild()?;
        self.set_meta(OWNER, owner)?;
        self.owner = Some(owner.to_owned());
        Ok(true)
    }

    /// The directory the cache is in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Set the cache up again from nothing (it was found damaged while in use), still the claimed
    /// owner's. The writer only.
    pub fn rebuild(&mut self) -> Result<(), Fault> {
        if self.read_only() {
            return Err(Fault::other("cache: read-only, not rebuilt"));
        }
        let gone = std::mem::replace(&mut self.conn, Connection::open_in_memory().map_err(|e| fault(&e))?);
        drop(gone);
        remove_files(&self.dir)?;
        self.conn = open_writer(&self.dir.join(FILE)).map_err(Unusable::into_fault)?;
        match &self.owner {
            Some(owner) => self.set_meta(OWNER, owner),
            None => Ok(()),
        }
    }

    /// A value the cache keeps about itself.
    pub fn meta(&self, key: &str) -> Result<Option<String>, Fault> {
        use rusqlite::OptionalExtension;
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .map_err(|e| fault(&e))
    }

    /// Keep `value` under `key` (the writer only; read-only, nothing happens).
    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), Fault> {
        if self.read_only() {
            return Ok(());
        }
        self.conn
            .execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = ?2",
                [key, value],
            )
            .map(drop)
            .map_err(|e| fault(&e))
    }
}

/// Why a database cannot be used as the writer's.
enum Unusable {
    /// Not a database, damaged, or a newer version's: set it up again.
    Damaged,
    Fault(Fault),
}

impl Unusable {
    fn into_fault(self) -> Fault {
        match self {
            Unusable::Damaged => Fault::other("cache: damaged right after it was made"),
            Unusable::Fault(f) => f,
        }
    }
}

fn sql(e: &rusqlite::Error) -> Unusable {
    if is_corrupt(e) { Unusable::Damaged } else { Unusable::Fault(fault(e)) }
}

/// Take the writer's `lock`, trying again for `wait` while another holds it. `false`: another
/// still holds it.
fn locked(lock: &File, wait: Duration) -> std::io::Result<bool> {
    let deadline = Instant::now() + wait;
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(true),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => std::thread::sleep(LOCK_STEP),
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Error(e)) => return Err(e),
        }
    }
}

/// The database at `path` for writing, at the current schema.
fn open_writer(path: &Path) -> Result<Connection, Unusable> {
    private_file(path).map_err(|e| Unusable::Fault(io_fault(&e, path)))?;
    for side in SIDE {
        let p = side_path(path, side);
        private_side(&p).map_err(|e| Unusable::Fault(io_fault(&e, &p)))?;
    }
    let mut conn = Connection::open(path).map_err(|e| sql(&e))?;
    conn.busy_timeout(Duration::from_secs(2)).map_err(|e| sql(&e))?;
    // On a new file only (it takes effect before the first table): deleted rows give their
    // pages back to the file system when asked (`PRAGMA incremental_vacuum` after a prune).
    conn.pragma_update(None, "auto_vacuum", "INCREMENTAL").map_err(|e| sql(&e))?;
    let mode =
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get::<_, String>(0)).map_err(|e| sql(&e))?;
    wal(&mode).map_err(Unusable::Fault)?;
    conn.pragma_update(None, "synchronous", "NORMAL").map_err(|e| sql(&e))?;
    match schema::migrate(&mut conn, MIGRATIONS) {
        Ok(Schema::Current) => Ok(conn),
        Ok(Schema::Newer(_)) => Err(Unusable::Damaged),
        Err(e) => Err(sql(&e)),
    }
}

/// The journal mode SQLite answered is WAL, which the one writer and its readers need (a file
/// system without it: the app runs without the cache, and the error log says why).
fn wal(mode: &str) -> Result<(), Fault> {
    if mode.eq_ignore_ascii_case("wal") {
        Ok(())
    } else {
        Err(Fault::other(format!("cache: the journal mode is {mode}, not WAL; not used")))
    }
}

/// The database at `path` for reading only, when it is there (a file, not a link) and of this
/// version.
fn open_read_only(path: &Path) -> Option<Connection> {
    if !std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file()) {
        return None;
    }
    let conn =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    conn.busy_timeout(Duration::from_secs(2)).ok()?;
    (schema::version(&conn).ok()? as usize == MIGRATIONS.len()).then_some(conn)
}

fn side_path(path: &Path, side: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(side);
    PathBuf::from(s)
}

/// Remove the database and its journal files in `dir` (the lock stays: [`remove`] takes it):
/// `slakio logout`, and a cache set up again. What is not there is not an error.
pub fn remove_files(dir: &Path) -> Result<(), Fault> {
    let db = dir.join(FILE);
    for p in [db.clone(), side_path(&db, SIDE[0]), side_path(&db, SIDE[1])] {
        match std::fs::remove_file(&p) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io_fault(&e, &p)),
            _ => {}
        }
    }
    Ok(())
}

/// Remove the cache of the cache directory `parent`: [`remove_files`], the lock file and the
/// cache's own directory: nothing of it is left (a link in its place goes, never what it
/// names). `slakio logout`. The files a development version kept in `parent` itself go too.
pub fn remove(parent: &Path) -> Result<(), Fault> {
    let dir = parent.join(DIR);
    let gone = |r: std::io::Result<()>, p: &Path| match r {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io_fault(&e, p)),
        _ => Ok(()),
    };
    remove_files(parent)?;
    let old_lock = parent.join(LOCK);
    gone(std::fs::remove_file(&old_lock), &old_lock)?;
    match std::fs::symlink_metadata(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Ok(m) if !m.is_dir() => return gone(std::fs::remove_file(&dir), &dir),
        _ => {}
    }
    remove_files(&dir)?;
    let lock = dir.join(LOCK);
    gone(std::fs::remove_file(&lock), &lock)?;
    gone(std::fs::remove_dir(&dir), &dir)
}

/// Another process holds the writer's lock of the cache in the cache directory `parent`: a
/// slakio runs and uses it.
pub fn in_use(parent: &Path) -> bool {
    let mut opts = OpenOptions::new();
    opts.read(true).write(true);
    no_follow(&mut opts);
    let Ok(lock) = opts.open(parent.join(DIR).join(LOCK)) else { return false };
    matches!(lock.try_lock(), Err(TryLockError::WouldBlock))
}

/// The cache's own directory in `parent` (both made if missing; `parent` left as it is),
/// readable by the user only (Unix), as an absolute path. Anything there but a directory (a
/// symbolic link, a file) is refused.
fn own_dir(parent: &Path) -> Result<PathBuf, Fault> {
    std::fs::create_dir_all(parent).map_err(|e| io_fault(&e, parent))?;
    let dir = std::path::absolute(parent.join(DIR)).map_err(|e| io_fault(&e, parent))?;
    #[cfg(unix)]
    let make = {
        let mut make = std::fs::DirBuilder::new();
        std::os::unix::fs::DirBuilderExt::mode(&mut make, 0o700);
        make
    };
    #[cfg(not(unix))]
    let make = std::fs::DirBuilder::new();
    match make.create(&dir) {
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(io_fault(&e, &dir)),
        _ => {}
    }
    if !std::fs::symlink_metadata(&dir).map_err(|e| io_fault(&e, &dir))?.is_dir() {
        return Err(Fault::other("cache: its directory is a link or not a directory; not used"));
    }
    make_dir_private(&dir).map_err(|e| io_fault(&e, &dir))?;
    Ok(dir)
}

#[cfg(unix)]
fn make_dir_private(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn make_dir_private(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

/// The file at `path`, made if missing (never readable by others, not even for a moment), and
/// made private if it was not. A symbolic link there is refused.
fn private_file(path: &Path) -> std::io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    no_follow(&mut opts);
    let file = opts.open(path)?;
    make_private(&file)?;
    Ok(file)
}

/// SQLite's file at `path`, when it is there, made private; a symbolic link there is refused.
fn private_side(path: &Path) -> std::io::Result<()> {
    let mut opts = OpenOptions::new();
    opts.read(true).write(true);
    no_follow(&mut opts);
    match opts.open(path) {
        Ok(file) => make_private(&file),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Open without following a symbolic link (Unix; the open fails on one).
#[cfg(unix)]
fn no_follow(opts: &mut OpenOptions) {
    std::os::unix::fs::OpenOptionsExt::custom_flags(opts, libc::O_NOFOLLOW);
}

#[cfg(not(unix))]
fn no_follow(_opts: &mut OpenOptions) {}

/// The open `file` readable by the user only (through the file, never a path).
#[cfg(unix)]
fn make_private(file: &File) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn make_private(_file: &File) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
