//! The file the tabs and panes are saved in between runs (`restore = "on"`, the default): one per
//! set of workspaces, in the state directory — `layout-demo.toml` for the demo, else
//! `layout-<16 hex digits>.toml` named by the signed-in workspaces' ids (a hash, so no id is in
//! a file name). Its form is [`slakio_core::layout::saved`]'s.
//!
//! Read once at the start: a missing file is no layout; one that cannot be read whole (not
//! text, another version, a symbolic link, too big) is not used, and the app says so once.
//! Written whenever the layout changes, at most once a second, and at the end of the run, by an
//! atomic write private to the user (0600 on Unix), so a crash leaves the old file or the new
//! one, never half of one. Nothing in it is a secret: conversation ids, thread timestamps, tab
//! names.

use slakio_core::fault::Fault;
use slakio_core::fsutil::atomic_write_private;
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
}

/// The name of the file for the workspaces `ids` (in any order; the demo: `None`).
pub fn file_name(ids: Option<&[WorkspaceId]>) -> String {
    let Some(ids) = ids else { return "layout-demo.toml".to_string() };
    let mut ids: Vec<&str> = ids.iter().map(WorkspaceId::as_str).collect();
    ids.sort_unstable();
    ids.dedup();
    // FNV-1a, 64 bits: stable across runs and builds.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in ids.join("\n").bytes() {
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
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(Fault::io_at(&e, &path)),
        };
        if !meta.is_file() {
            return Err(Fault::other("the layout file is not a plain file"));
        }
        if meta.len() > MAX_BYTES {
            return Err(Fault::other(format!("the layout file is over {MAX_BYTES} bytes")));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| Fault::io_at(&e, &path))?;
        let saved = Saved::parse(&text).map_err(|why| Fault::other(format!("layout file: {why}")))?;
        self.written = Some(text);
        Ok(Some(saved))
    }

    /// The layout is `saved` at `now`: written when it changed, unless a write was made less
    /// than [`EVERY`] ago (then it waits for [`LayoutFile::deadline`], or the end).
    pub fn offer(&mut self, saved: Option<&Saved>, now: Instant) -> Result<(), Fault> {
        let Some(text) = saved.map(Saved::to_text) else { return Ok(()) };
        if self.written.as_ref() == Some(&text) {
            self.waiting = None;
            return Ok(());
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
        // A symbolic link put where the file goes is never written through.
        let result = if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            Err(Fault::other("the layout file is a symbolic link"))
        } else {
            atomic_write_private(&path, text.as_bytes()).map_err(|e| Fault::io_at(&e, &path))
        };
        self.broken = result.is_err();
        if result.is_ok() {
            self.written = Some(text);
        }
        result
    }
}

#[cfg(test)]
mod tests;
