//! Opening the cache: one SQLite file in the cache directory, private to the user (the directory
//! `0700`, the database and its journal files `0600`; SQLite gives its `-wal` and `-shm` files
//! the database file's mode), with one process writing it at a time.
//!
//! The writer holds an OS lock on `cache.lock` (`File::try_lock`: `flock` on Unix, `LockFileEx`
//! on Windows), released when the process ends however it ends. A second slakio while one runs
//! opens the database read-only: it draws from the cache but never writes it
//! ([`Store::read_only`]); when there is nothing it could read it does without
//! ([`OpenError::InUse`]).
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
use std::time::Duration;

/// The database's file name in the cache directory.
pub const FILE: &str = "cache.sqlite3";
/// The writer's lock.
const LOCK: &str = "cache.lock";
/// SQLite's files beside the database.
const SIDE: [&str; 2] = ["-wal", "-shm"];

/// The cache, open.
pub struct Store {
    pub(crate) conn: Connection,
    dir: PathBuf,
    /// Held while this process writes the cache (`None`: read-only).
    lock: Option<File>,
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
    /// Open the cache in `dir`, made if missing.
    pub fn open(dir: &Path) -> Result<Opened, OpenError> {
        private_dir(dir).map_err(OpenError::Failed)?;
        let lock_path = dir.join(LOCK);
        let lock = private_file(&lock_path).map_err(|e| OpenError::Failed(io_fault(&e, &lock_path)))?;
        let path = dir.join(FILE);
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                let conn = open_read_only(&path).ok_or(OpenError::InUse)?;
                return Ok(Opened { store: Store { conn, dir: dir.to_path_buf(), lock: None }, rebuilt: false });
            }
            Err(TryLockError::Error(e)) => return Err(OpenError::Failed(io_fault(&e, &lock_path))),
        }
        let (conn, rebuilt) = match open_writer(&path) {
            Ok(conn) => (conn, false),
            Err(Unusable::Damaged) => {
                remove_files(dir).map_err(OpenError::Failed)?;
                (open_writer(&path).map_err(Unusable::into_fault).map_err(OpenError::Failed)?, true)
            }
            Err(Unusable::Fault(f)) => return Err(OpenError::Failed(f)),
        };
        Ok(Opened { store: Store { conn, dir: dir.to_path_buf(), lock: Some(lock) }, rebuilt })
    }

    /// Another process writes the cache: this one only reads it.
    pub fn read_only(&self) -> bool {
        self.lock.is_none()
    }

    /// The directory the cache is in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Set the cache up again from nothing (it was found damaged while in use). The writer only.
    pub fn rebuild(&mut self) -> Result<(), Fault> {
        if self.read_only() {
            return Err(Fault::other("cache: read-only, not rebuilt"));
        }
        let gone = std::mem::replace(&mut self.conn, Connection::open_in_memory().map_err(|e| fault(&e))?);
        drop(gone);
        remove_files(&self.dir)?;
        self.conn = open_writer(&self.dir.join(FILE)).map_err(Unusable::into_fault)?;
        Ok(())
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

/// The database at `path` for writing, at the current schema.
fn open_writer(path: &Path) -> Result<Connection, Unusable> {
    private_file(path).map_err(|e| Unusable::Fault(io_fault(&e, path)))?;
    for side in SIDE {
        let p = side_path(path, side);
        if p.exists() {
            make_private(&p).map_err(|e| Unusable::Fault(io_fault(&e, &p)))?;
        }
    }
    let mut conn = Connection::open(path).map_err(|e| sql(&e))?;
    conn.busy_timeout(Duration::from_secs(2)).map_err(|e| sql(&e))?;
    // On a new file only (it takes effect before the first table): deleted rows give their
    // pages back to the file system when asked (`PRAGMA incremental_vacuum` after a prune).
    conn.pragma_update(None, "auto_vacuum", "INCREMENTAL").map_err(|e| sql(&e))?;
    conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get::<_, String>(0)).map_err(|e| sql(&e))?;
    conn.pragma_update(None, "synchronous", "NORMAL").map_err(|e| sql(&e))?;
    match schema::migrate(&mut conn, MIGRATIONS) {
        Ok(Schema::Current) => Ok(conn),
        Ok(Schema::Newer(_)) => Err(Unusable::Damaged),
        Err(e) => Err(sql(&e)),
    }
}

/// The database at `path` for reading only, when it is there and of this version.
fn open_read_only(path: &Path) -> Option<Connection> {
    if !path.is_file() {
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

/// Remove the cache in `dir` (the database, its journal files and the lock): `slakio logout`,
/// and a cache set up again. What is not there is not an error.
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

/// [`remove_files`], and the lock file: nothing of the cache is left.
pub fn remove(dir: &Path) -> Result<(), Fault> {
    remove_files(dir)?;
    let lock = dir.join(LOCK);
    match std::fs::remove_file(&lock) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io_fault(&e, &lock)),
        _ => Ok(()),
    }
}

/// `dir`, made if missing, readable by the user only (Unix).
fn private_dir(dir: &Path) -> Result<(), Fault> {
    std::fs::create_dir_all(dir).map_err(|e| io_fault(&e, dir))?;
    make_dir_private(dir).map_err(|e| io_fault(&e, dir))
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
/// made private if it was not.
fn private_file(path: &Path) -> std::io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let file = opts.open(path)?;
    make_private(path)?;
    Ok(file)
}

#[cfg(unix)]
fn make_private(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn make_private(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
