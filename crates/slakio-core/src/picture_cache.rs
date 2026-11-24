//! The pictures kept on disk between runs (`<cache>/pictures`,
//! [`crate::paths::Paths::pictures_cache`]): the small PNG made for the terminal from a profile
//! photo, a workspace's own emoji or a file's preview, so the next run draws it without asking
//! the network (and without counting toward the request governor's picture share).
//!
//! * A file is named by [`key`] alone (its first 16 bytes in hex, then `.png`; the key is the
//!   SHA-256 of the kind, the address and the cells it fills): never the address, a name or an
//!   id. It holds a header ([`MAGIC`], all 32 bytes of the key, the time it was written, the
//!   kind) and the PNG, nothing else. A file of an earlier format is refused and removed.
//! * The folder is private (0700) and each file too (0600), written whole or not at all, never
//!   through a link; a folder that is a link, or not the user's own, or one others may write in,
//!   is neither read nor written. It is made once, when
//!   a run starts ([`PictureCache::open`]), never by a write: one removed while slakio runs
//!   (`slakio logout`) stays removed, and nothing more is written.
//! * A file is read only when it is a plain file (not a link, opened without following one) of
//!   at most [`FILE_MAX`] bytes, with the header of the key asked for, written no later than a day
//!   from now and a PNG after it; anything else is a miss and the file is removed. What is read
//!   is untrusted all the same: the UI decodes it within the bounds of a fetched picture.
//! * A file is too old after [`AVATAR_AGE`] (photos), [`EMOJI_AGE`] (emoji, whose addresses
//!   change with the picture) or [`THUMB_AGE`] (previews): a miss, and removed.
//! * At most [`BUDGET`] bytes in all: [`PictureCache::sweep`] (at the start, and whenever a write
//!   takes the total over) removes what is too old, then, once the folder holds more than that,
//!   the least recently used (by the time a file was last read or written, set here) until the
//!   rest takes at most four fifths of it: the next sweep is many writes away. A file written
//!   again counts once.
//!
//! Everything here reads or writes the disk: never on the UI's thread.

use crate::images::Source;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The folder's name in the cache directory.
pub const DIR: &str = "pictures";
/// The most bytes the folder holds.
pub const BUDGET: u64 = 64 * 1024 * 1024;
/// The most bytes one file holds (header and PNG); a larger picture is not kept.
pub const FILE_MAX: u64 = 1024 * 1024;
/// How long a profile photo is kept.
pub const AVATAR_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
/// How long a workspace's own emoji is kept.
pub const EMOJI_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
/// How long a file's preview is kept.
pub const THUMB_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
/// The first bytes of every file (the format's version is its last digit).
pub const MAGIC: &[u8; 8] = b"slkpic2\n";
/// The header: [`MAGIC`], the key (32 bytes), the time written (a little-endian `u64`), the
/// kind.
const HEADER: usize = 49;
const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
/// How far after now a file's time may be (a clock put back a little) before it is refused.
const SKEW: Duration = Duration::from_secs(24 * 3600);
/// A write's temporary file still there after this long was left by a run that ended.
const TMP_AGE: Duration = Duration::from_secs(3600);

/// SHA-256 of `parts`, each after its length (a little-endian `u64`), so no two lists of parts
/// hash the same bytes: the same on every machine, Rust version and run, and two pictures never
/// share a file.
pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    for part in parts {
        ctx.update(&(part.len() as u64).to_le_bytes());
        ctx.update(part);
    }
    let mut key = [0; 32];
    key.copy_from_slice(ctx.finish().as_ref());
    key
}

/// The kind as one byte of the header.
fn kind_byte(source: &Source) -> u8 {
    match source {
        Source::Thumb(_) => 0,
        Source::Avatar(_) => 1,
        Source::Emoji(_) => 2,
    }
}

/// How long a picture of the kind `byte` is kept (`None`: not a kind).
fn age_of(byte: u8) -> Option<Duration> {
    [THUMB_AGE, AVATAR_AGE, EMOJI_AGE].get(usize::from(byte)).copied()
}

/// The key of the picture of `source` that fills the cells `shape` names.
pub fn key(source: &Source, shape: &str) -> [u8; 32] {
    sha256(&[b"slakio-picture", &[kind_byte(source)], source.address().as_bytes(), shape.as_bytes()])
}

/// The file's name: the key's first 16 bytes in hex, then `.png`.
pub fn file_name(source: &Source, shape: &str) -> String {
    let hex: String = key(source, shape)[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("{hex}.png")
}

/// `name` is one [`file_name`] makes.
fn is_name(name: &str) -> bool {
    name.len() == 36
        && name.ends_with(".png")
        && name.bytes().take(32).all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// What time it is (tests set it).
pub type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// What reading a picture gave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loaded {
    /// The PNG kept for it.
    Hit(Vec<u8>),
    /// Not kept.
    Miss,
    /// Kept, but too old (removed now).
    Expired,
    /// A file that is not one this writes (removed now), in a word.
    Refused(&'static str),
}

/// What writing a picture did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stored {
    /// Written; the sweep it set off when the folder went over its room.
    Stored(Option<Swept>),
    /// Not written, in a word.
    Refused(&'static str),
}

/// What a sweep did: files removed, files kept, the bytes they hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Swept {
    pub removed: usize,
    pub kept: usize,
    pub bytes: u64,
}

/// The picture cache in one folder. Cloned freely: the clones share the count of bytes held.
#[derive(Clone)]
pub struct PictureCache {
    dir: PathBuf,
    budget: u64,
    now: Clock,
    /// The bytes in the folder, as the last sweep found and writes since added.
    total: Arc<AtomicU64>,
    sweeping: Arc<Mutex<()>>,
    /// The folder was made or found at the start ([`PictureCache::open`]): writes may follow.
    opened: Arc<AtomicBool>,
}

impl fmt::Debug for PictureCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PictureCache").field("dir", &self.dir).field("budget", &self.budget).finish()
    }
}

/// Remove what is at `path` (a file or a link; a folder only when empty). Best effort.
fn remove(path: &Path) {
    if fs::remove_file(path).is_err() {
        let _ = fs::remove_dir(path);
    }
}

/// Opened without following a link and without waiting on it (Unix).
fn open_plain(path: &Path, write: bool) -> io::Result<fs::File> {
    let mut opts = fs::OpenOptions::new();
    opts.read(!write).write(write);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut opts, libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let f = opts.open(path)?;
    if !f.metadata()?.is_file() {
        return Err(io::Error::other("not a plain file"));
    }
    Ok(f)
}

/// The file's last use is `at` (the sweep removes the least recently used first).
fn touch(path: &Path, at: SystemTime) {
    if let Ok(f) = open_plain(path, true) {
        let _ = f.set_modified(at);
    }
}

impl PictureCache {
    /// The cache in `dir` ([`PictureCache::open`] makes it), [`BUDGET`] bytes, the system's
    /// clock.
    pub fn new(dir: PathBuf) -> Self {
        let (total, sweeping, opened) = (Arc::default(), Arc::default(), Arc::default());
        Self { dir, budget: BUDGET, now: Arc::new(SystemTime::now), total, sweeping, opened }
    }

    /// Make the folder (private) or find it, at the start of a run: only then are pictures
    /// written, and never again is it made. `Some(true)` when it was made now, `None` when it
    /// cannot be used (not a folder, a link, not made).
    pub fn open(&self) -> Option<bool> {
        let fresh = match fs::symlink_metadata(&self.dir) {
            Ok(m) if !m.is_dir() => return None,
            Ok(_) => false,
            Err(e) if e.kind() == io::ErrorKind::NotFound => true,
            Err(_) => return None,
        };
        if !crate::fsutil::own_private_dir(&self.dir).unwrap_or(false) || !self.dir_ok() {
            return None;
        }
        self.opened.store(true, Ordering::Relaxed);
        Some(fresh)
    }

    /// The same, holding at most `budget` bytes.
    pub fn with_budget(mut self, budget: u64) -> Self {
        self.budget = budget;
        self
    }

    /// The same, telling the time with `now`.
    pub fn with_clock(mut self, now: Clock) -> Self {
        self.now = now;
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file of the picture of `source` as `shape`.
    pub fn path(&self, source: &Source, shape: &str) -> PathBuf {
        self.dir.join(file_name(source, shape))
    }

    /// The folder is there, a folder (not a link to one), the user's own and no one else's to
    /// write in.
    fn dir_ok(&self) -> bool {
        crate::fsutil::is_own_dir(&self.dir)
    }

    /// The PNG kept for `source` as `shape`, when there is one that may be read; a file that is
    /// too old or refused is removed. A hit counts as a use (its time is set to now).
    pub fn load(&self, source: &Source, shape: &str) -> Loaded {
        if !self.dir_ok() {
            return Loaded::Miss;
        }
        let path = self.path(source, shape);
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Loaded::Miss,
            Err(_) => return Loaded::Refused("unreadable"),
        };
        let refused = |why| {
            remove(&path);
            Loaded::Refused(why)
        };
        if meta.file_type().is_symlink() {
            return refused("link");
        }
        if !meta.is_file() {
            return refused("not-file");
        }
        if meta.len() > FILE_MAX {
            return refused("too-large");
        }
        let Ok(bytes) = crate::fsutil::read_plain(&path, FILE_MAX) else { return refused("unreadable") };
        let now = (self.now)();
        match parse(&bytes, source, shape, now) {
            Err(why) => refused(why),
            Ok(None) => {
                remove(&path);
                Loaded::Expired
            }
            Ok(Some(png)) => {
                touch(&path, now);
                Loaded::Hit(png)
            }
        }
    }

    /// Remove what is kept for `source` as `shape` (one the UI could not decode). Best effort.
    pub fn forget(&self, source: &Source, shape: &str) {
        if self.dir_ok() {
            remove(&self.path(source, shape));
        }
    }

    /// Keep `png`, the picture of `source` as `shape`, in the folder opened at the start (one
    /// gone since is not made again). A write that takes the folder over its room sweeps it.
    pub fn store(&self, source: &Source, shape: &str, png: &[u8]) -> Stored {
        if (HEADER + png.len()) as u64 > FILE_MAX {
            return Stored::Refused("too-large");
        }
        if !png.starts_with(&PNG) {
            return Stored::Refused("not-png");
        }
        if !self.opened.load(Ordering::Relaxed) || !self.dir_ok() {
            return Stored::Refused("folder");
        }
        let now = (self.now)();
        let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let mut bytes = Vec::with_capacity(HEADER + png.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&key(source, shape));
        bytes.extend_from_slice(&secs.to_le_bytes());
        bytes.push(kind_byte(source));
        bytes.extend_from_slice(png);
        let path = self.path(source, shape);
        // A file written again replaces the one counted.
        let old = fs::symlink_metadata(&path).ok().filter(fs::Metadata::is_file).map_or(0, |m| m.len());
        if crate::fsutil::atomic_replace_private_nofollow(&path, &bytes).is_err() {
            return Stored::Refused("write");
        }
        touch(&path, now);
        let len = bytes.len() as u64;
        let next = |t: u64| t.saturating_add(len).saturating_sub(old);
        let total = next(self.total.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |t| Some(next(t))).unwrap_or(0));
        Stored::Stored(if total > self.budget { self.sweep() } else { None })
    }

    /// Remove what is too old, not a file this writes, or a write's leftover, then, when the rest
    /// does not fit in the room, the least recently used until four fifths of it are left. `None` when another sweep runs or the
    /// folder is not one (a link is never read).
    pub fn sweep(&self) -> Option<Swept> {
        let _one = match self.sweeping.try_lock() {
            Ok(one) => one,
            // A sweep that panicked left nothing half done: the next one runs.
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
            Err(TryLockError::WouldBlock) => return None,
        };
        if !self.dir_ok() {
            return None;
        }
        let now = (self.now)();
        let mut swept = Swept::default();
        let mut kept: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
        for entry in fs::read_dir(&self.dir).ok()?.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(meta) = fs::symlink_metadata(&path) else { continue };
            let used = meta.modified().unwrap_or(UNIX_EPOCH);
            let gone = if name.starts_with('.') && name.contains(".tmp-") && meta.is_file() {
                now.duration_since(used).is_ok_and(|d| d > TMP_AGE)
            } else if meta.is_dir() && !is_name(&name) {
                // A folder this never makes: left as it is.
                false
            } else {
                !meta.is_file() || !is_name(&name) || meta.len() > FILE_MAX || stale(&path, now)
            };
            if gone {
                remove(&path);
                swept.removed += 1;
            } else if meta.is_file() && is_name(&name) {
                kept.push((used, meta.len(), path));
            }
        }
        kept.sort_by_key(|k| k.0);
        let mut bytes: u64 = kept.iter().map(|k| k.1).sum();
        let mut oldest = kept.iter();
        let low = if bytes > self.budget { self.budget / 5 * 4 } else { bytes };
        while bytes > low {
            let Some((_, len, path)) = oldest.next() else { break };
            remove(path);
            bytes -= len;
            swept.removed += 1;
        }
        swept.kept = oldest.count();
        swept.bytes = bytes;
        self.total.store(bytes, Ordering::Relaxed);
        Some(swept)
    }

    /// Remove the folder and everything in it (a link in its place: the link only).
    pub fn clear(&self) -> io::Result<()> {
        match fs::symlink_metadata(&self.dir) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
            Ok(m) if m.is_dir() => fs::remove_dir_all(&self.dir),
            Ok(_) => fs::remove_file(&self.dir),
        }
    }
}

/// The PNG of a file's `bytes` for `source` as `shape` (`None`: too old at `now`), or why
/// the file is refused.
fn parse(bytes: &[u8], source: &Source, shape: &str, now: SystemTime) -> Result<Option<Vec<u8>>, &'static str> {
    let (age, written) = header(bytes).ok_or("header")?;
    if bytes[8..40] != key(source, shape) || bytes[48] != kind_byte(source) {
        return Err("key");
    }
    if written > now + SKEW {
        return Err("future");
    }
    if now.duration_since(written).unwrap_or_default() > age {
        return Ok(None);
    }
    let png = &bytes[HEADER..];
    if !png.starts_with(&PNG) {
        return Err("not-png");
    }
    Ok(Some(png.to_vec()))
}

/// How long the file whose first bytes are `bytes` is kept and when it was written, when its
/// header is one this writes.
fn header(bytes: &[u8]) -> Option<(Duration, SystemTime)> {
    if bytes.len() < HEADER || &bytes[..8] != MAGIC {
        return None;
    }
    let secs = u64::from_le_bytes(bytes[40..48].try_into().ok()?);
    Some((age_of(bytes[48])?, UNIX_EPOCH.checked_add(Duration::from_secs(secs))?))
}

/// The file at `path` is too old at `now`, or its header is not one this writes.
fn stale(path: &Path, now: SystemTime) -> bool {
    let mut first = Vec::with_capacity(HEADER);
    let read = open_plain(path, false).and_then(|f| f.take(HEADER as u64).read_to_end(&mut first));
    match read.ok().and_then(|_| header(&first)) {
        Some((age, written)) => written > now + SKEW || now.duration_since(written).unwrap_or_default() > age,
        None => true,
    }
}

#[cfg(test)]
mod tests;
