//! Why something failed, as data: a [`FaultKind`] the UI turns into a short message in the
//! user's language, and the raw `detail` (the OS's or a parser's text) that only goes to the
//! error log ([`ErrorLog`]). Core never renders a fault for the UI itself.
//!
//! Messages from Slack are not faults: they are data and are shown as they are.

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// The kind of a failure, as far as the UI needs to tell it apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FaultKind {
    /// A file operation failed.
    Io(io::ErrorKind),
    /// A TOML file does not parse (or a value has the wrong type); the line (1-based) when
    /// known.
    Toml { line: Option<usize> },
    /// A secret store failed (locked, missing, refused, or did not answer).
    SecretStore,
    /// Anything else; only the detail says what.
    Other,
}

/// A failure: its kind, and the raw text for the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub kind: FaultKind,
    /// The raw text (OS, parser, platform), for the error log only.
    pub detail: String,
}

impl Fault {
    pub fn new(kind: FaultKind, detail: impl Into<String>) -> Self {
        Self { kind, detail: detail.into() }
    }

    /// A failed file operation.
    pub fn io(e: &io::Error) -> Self {
        Self::new(FaultKind::Io(e.kind()), e.to_string())
    }

    /// A failed file operation on `path` (the path goes into the detail).
    pub fn io_at(e: &io::Error, path: &Path) -> Self {
        Self::new(FaultKind::Io(e.kind()), format!("{}: {e}", path.display()))
    }

    /// `text` does not parse as TOML (or has a value of the wrong type): the line of `span`.
    pub fn toml(text: &str, span: Option<std::ops::Range<usize>>, message: &str) -> Self {
        let line = span.map(|s| text[..s.start.min(text.len())].matches('\n').count() + 1);
        Self::new(FaultKind::Toml { line }, message.trim().to_string())
    }

    /// A `toml_edit` error of `text`.
    pub fn toml_edit(text: &str, e: &toml_edit::TomlError) -> Self {
        Self::toml(text, e.span(), e.message())
    }

    /// Anything else.
    pub fn other(detail: impl Into<String>) -> Self {
        Self::new(FaultKind::Other, detail)
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

/// Tests build faults from plain text (the kind is [`FaultKind::Other`]).
#[cfg(test)]
impl From<&str> for Fault {
    fn from(detail: &str) -> Self {
        Self::other(detail)
    }
}

impl From<io::Error> for Fault {
    fn from(e: io::Error) -> Self {
        Self::io(&e)
    }
}

/// `<state>/errors.log`: the detail of every failure the UI reported in words, one line each
/// (`<unix seconds> <context>: <kind>: <detail>`), so nothing is lost by keeping the status line
/// short. The detail is redacted first ([`redact::redact`]): no token, cookie, address past its
/// host or quoted text reaches the file. The file is private to the user (mode 0600, its
/// directory 0700 when made here; never written through a link) and at most [`LOG_MAX`] bytes:
/// past that it becomes `errors.log.1` (the one before is dropped) and a new one starts.
/// Appending is best effort; without a state directory nothing is written.
#[derive(Clone, Debug, Default)]
pub struct ErrorLog {
    path: Option<PathBuf>,
}

/// The most bytes one error log file holds before it is rotated.
pub const LOG_MAX: u64 = 1024 * 1024;

impl ErrorLog {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self { path }
    }

    /// Where it writes.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Append `fault` as having happened while `context` (a message key).
    pub fn record(&self, context: &str, fault: &Fault) {
        let Some(path) = &self.path else { return };
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let detail = redact::redact(&fault.detail);
        // A fault without a detail says so instead of ending the line after its kind.
        let line = match detail.trim() {
            "" => format!("{secs} {}: {:?} (no detail given)\n", redact::redact(context), fault.kind),
            _ => format!("{secs} {}: {:?}: {detail}\n", redact::redact(context), fault.kind),
        };
        let _ = append(path, line.as_bytes(), LOG_MAX);
    }
}

/// Append `bytes` to the private file at `path`, rotating it first when it would grow past `max`.
fn append(path: &Path, bytes: &[u8], max: u64) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        let mut b = std::fs::DirBuilder::new();
        b.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        b.create(dir)?;
        // One made looser before (or by hand) is made private too, when it is the user's (only
        // its owner may change its mode; otherwise it stays as it is).
        #[cfg(unix)]
        if std::fs::metadata(dir).is_ok_and(|m| m.permissions().mode() & 0o077 != 0) {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    match std::fs::symlink_metadata(path) {
        // A link (or anything but a file) is not written through.
        Ok(m) if !m.file_type().is_file() => return Err(io::Error::other("the error log is not a plain file")),
        Ok(m) if m.len() + bytes.len() as u64 > max => {
            let mut old = path.as_os_str().to_owned();
            old.push(".1");
            let old = PathBuf::from(old);
            // An older version's file may be readable by others: private before it moves, and
            // after (nothing else may have made it looser meanwhile).
            make_private(path)?;
            std::fs::rename(path, &old)?;
            make_private(&old)?;
        }
        _ => {}
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    // Never through a link, not even one put there after the check above.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut f = opts.open(path)?;
    // A file made by an older version may be readable by others: no longer.
    #[cfg(unix)]
    if f.metadata()?.permissions().mode() & 0o077 != 0 {
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(bytes)
}

/// `path` readable and writable by its owner only (Unix; elsewhere nothing changes).
fn make_private(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub mod redact;

#[cfg(test)]
mod tests;
