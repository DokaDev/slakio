//! Keeping the cache out of backups. On macOS the cache's own directory is excluded from Time Machine
//! with `tmutil addexclusion <dir>`, which marks the directory itself (a sticky exclusion, kept in
//! its extended attributes): it holds for whichever volume or backup disk, and goes with the
//! directory. Done once per cache: once it worked, the cache remembers it, and later starts run
//! nothing.
//!
//! The step is behind [`Exclude`] so the tests use a fake and never touch Time Machine. Elsewhere
//! there is nothing to do ([`Exclude`] is not called).

use crate::db::Store;
use std::path::{Path, PathBuf};

/// Something that excludes a directory from backups.
pub trait Exclude {
    /// Exclude `dir`; the reason in a few words when it could not.
    fn exclude(&self, dir: &Path) -> Result<(), String>;
}

/// `tmutil addexclusion` (macOS), or a program of the same arguments.
#[derive(Clone, Debug)]
pub struct Tmutil {
    pub program: PathBuf,
}

impl Default for Tmutil {
    fn default() -> Self {
        Self { program: PathBuf::from("/usr/bin/tmutil") }
    }
}

impl Exclude for Tmutil {
    fn exclude(&self, dir: &Path) -> Result<(), String> {
        // An absolute path: never one that could read as an option (`-p`).
        let dir = std::path::absolute(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let out = std::process::Command::new(&self.program)
            .arg("addexclusion")
            .arg(&dir)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("{} did not start: {e}", self.program.display()))?;
        if out.status.success() {
            Ok(())
        } else {
            // tmutil's own words go to the error log only, cut to a line.
            let said: String =
                String::from_utf8_lossy(&out.stderr).chars().filter(|c| !c.is_control()).take(160).collect();
            Err(format!("{} addexclusion: {} {said}", self.program.display(), out.status))
        }
    }
}

/// The key under which the cache remembers the directory it excluded.
const EXCLUDED: &str = "backup.excluded";

impl Store {
    /// Exclude the cache's directory from backups with `how`, unless it was already (the writer
    /// only; read-only, nothing is done). `Ok(true)` when it was done now.
    pub fn exclude_from_backups(&self, how: &dyn Exclude) -> Result<bool, String> {
        let dir = self.dir().to_string_lossy().into_owned();
        if self.read_only() || self.meta(EXCLUDED).ok().flatten().as_deref() == Some(dir.as_str()) {
            return Ok(false);
        }
        how.exclude(self.dir())?;
        self.set_meta(EXCLUDED, &dir).map_err(|f| f.to_string())?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
