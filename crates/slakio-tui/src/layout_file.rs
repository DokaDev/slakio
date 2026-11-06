//! The file the tabs and panes are saved in between runs (`restore = "on"`, the default): one per
//! set of sign-ins, in the state directory — `layout-demo.toml` for the demo, else
//! `layout-<16 hex digits>.toml` named by the signed-in workspaces' ids and the user signed in
//! to each (a hash, so no id is in a file name; another user of a workspace has a file of
//! their own, as with the cache). Its form is [`slakio_core::layout::saved`]'s.
//!
//! Read once at the start: a missing file is no layout; one that cannot be read whole (not
//! text, another version, a symbolic link or anything but a plain file, too big) is not used,
//! and the app says so once; the first write moves it aside to `<name>.bak` (one kept), so a
//! file edited by hand is not lost. Written whenever the layout changes, at most once a second,
//! and at the end of the run, by an atomic write private to the user (0600 on Unix, in a state
//! directory made 0700), so a crash leaves the old file or the new one, never half of one; a
//! text that would not read back is never written. Nothing in it is a secret: conversation
//! ids, thread timestamps, tab names.

use slakio_core::fault::Fault;
use slakio_core::fsutil::{atomic_write_private, private_dir, read_plain};
use slakio_core::layout::saved::Saved;
use slakio_core::model::WorkspaceId;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The most a layout file may hold.
pub const MAX_BYTES: u64 = 1 << 20;
/// The least time between two writes while the layout changes (a border dragged).
pub const EVERY: Duration = Duration::from_secs(1);

/// The layout file of a run, and what was last written to it.
#[derive(Debug, Default)]
pub struct LayoutFile {
    path: Option<PathBuf>,
    /// The text on disk (as read or last written).
    written: Option<String>,
    written_at: Option<Instant>,
    /// A newer text waiting for its turn.
    waiting: Option<String>,
    /// A write failed: none is tried again this run (the failure is said once).
    broken: bool,
    /// The file there could not be read: it is moved aside before the first write.
    unreadable: bool,
    /// A layout that would not read back was refused (said once).
    refused: bool,
    /// The app's stamp of the layout last looked at ([`LayoutFile::keep`]).
    seen: Option<(u64, bool)>,
}

/// The name of the file for the sign-ins `owners` (workspace and the user's id, in any order;
/// the demo: `None`).
pub fn file_name(owners: Option<&[(WorkspaceId, String)]>) -> String {
    let Some(owners) = owners else { return "layout-demo.toml".to_string() };
    let mut keys: Vec<String> = owners.iter().map(|(t, u)| format!("{}\t{u}", t.as_str())).collect();
    keys.sort_unstable();
    keys.dedup();
    // FNV-1a, 64 bits: stable across runs and builds.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in keys.join("\n").bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("layout-{h:016x}.toml")
}

impl LayoutFile {
    /// The file `name` in the state directory `state` (none: nothing is read or written).
    pub fn new(state: Option<&Path>, name: &str) -> Self {
        Self { path: state.map(|s| s.join(name)), ..Self::default() }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The layout saved; `Ok(None)` when there is none; why not when it cannot be used.
    pub fn load(&mut self) -> Result<Option<Saved>, Fault> {
        let Some(path) = self.path.clone() else { return Ok(None) };
        let bytes = match read_plain(&path, MAX_BYTES) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                self.unreadable = true;
                return Err(Fault::io_at(&e, &path));
            }
        };
        let parsed = String::from_utf8(bytes)
            .map_err(|_| "not text".to_string())
            .and_then(|text| Saved::parse(&text).map(|saved| (text, saved)));
        let (text, saved) = parsed.map_err(|why| {
            self.unreadable = true;
            Fault::other(format!("layout file: {why}"))
        })?;
        self.written = Some(text);
        Ok(Some(saved))
    }

    /// The layout as the app has it at `now`: looked at (made by `saved`) only when the app's
    /// `stamp` moved since the last time, and then offered ([`LayoutFile::offer`]); `end` (the
    /// run ends) writes whatever waits. Without a file, nothing is looked at.
    pub fn keep(
        &mut self,
        stamp: (u64, bool),
        saved: impl FnOnce() -> Option<Saved>,
        now: Instant,
        end: bool,
    ) -> Result<(), Fault> {
        if self.path.is_none() {
            return Ok(());
        }
        let mut result = Ok(());
        if self.seen != Some(stamp) {
            self.seen = Some(stamp);
            result = self.offer(saved().as_ref(), now);
        }
        if end || self.deadline().is_some_and(|at| at <= now) {
            result = result.and(self.flush(now));
        }
        result
    }

    /// The layout is `saved` at `now`: written when it changed, unless a write was made less
    /// than [`EVERY`] ago (then it waits for [`LayoutFile::deadline`], or the end). A text that
    /// would not read back is not written (said once).
    pub fn offer(&mut self, saved: Option<&Saved>, now: Instant) -> Result<(), Fault> {
        let Some(text) = saved.map(Saved::to_text) else { return Ok(()) };
        if self.written.as_ref() == Some(&text) {
            self.waiting = None;
            return Ok(());
        }
        if let Err(why) = Saved::parse(&text) {
            self.waiting = None;
            return match std::mem::replace(&mut self.refused, true) {
                true => Ok(()),
                false => Err(Fault::other(format!("layout not written, it would not read back: {why}"))),
            };
        }
        self.waiting = Some(text);
        match self.written_at {
            Some(at) if now.duration_since(at) < EVERY => Ok(()),
            _ => self.flush(now),
        }
    }

    /// When the text waiting may be written.
    pub fn deadline(&self) -> Option<Instant> {
        self.waiting
            .as_ref()
            .filter(|_| !self.broken)
            .map(|_| self.written_at.map_or_else(Instant::now, |at| at + EVERY))
    }

    /// Write the text waiting, if any (the end of the run; a deadline passed).
    pub fn flush(&mut self, now: Instant) -> Result<(), Fault> {
        let (Some(path), Some(text)) = (self.path.clone(), self.waiting.take()) else { return Ok(()) };
        if self.broken {
            return Ok(());
        }
        self.written_at = Some(now);
        let result = self.write(&path, &text);
        self.broken = result.is_err();
        if result.is_ok() {
            self.written = Some(text);
        }
        result
    }

    /// `text` to `path`, in a private directory; a file there that could not be read is moved
    /// aside first. A symbolic link put where the file goes is never written through.
    fn write(&mut self, path: &Path, text: &str) -> Result<(), Fault> {
        let meta = std::fs::symlink_metadata(path);
        if meta.as_ref().is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Fault::other("the layout file is a symbolic link"));
        }
        if let Some(dir) = path.parent() {
            private_dir(dir).map_err(|e| Fault::io_at(&e, dir))?;
        }
        if std::mem::take(&mut self.unreadable) && meta.is_ok_and(|m| m.is_file()) {
            let mut bak = path.as_os_str().to_owned();
            bak.push(".bak");
            std::fs::rename(path, &bak).map_err(|e| Fault::io_at(&e, path))?;
        }
        atomic_write_private(path, text.as_bytes()).map_err(|e| Fault::io_at(&e, path))
    }
}

#[cfg(test)]
mod tests;
