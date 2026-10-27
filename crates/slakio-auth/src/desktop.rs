//! Signing slakio in from the Slack desktop app's own on-disk state, with no network call and
//! no browser copy-paste: the session `d` cookie (and `d-s`, when present) from the app's
//! encrypted Cookies database, the per-workspace session tokens from its Local Storage, and the
//! signed-in workspaces (team id, name, host) from `storage/root-state.json`.
//!
//! The owner's real data is read only when the owner runs `slakio login` on their own machine:
//! the cookie database is copied to a temp file before it is opened (so the live profile is
//! never locked or mutated), the Safe Storage key is read from the login keychain (macOS shows
//! its own permission prompt), and every secret buffer is wiped from memory when dropped. No
//! secret is ever put into an error, a log line or a `Debug` print.
//!
//! macOS is the only platform implemented. On Linux and Windows [`scan`] compiles and returns
//! [`DesktopError::NotSupported`]; the config-directory matrix for those platforms is recorded
//! (see `configdir`) so the port can be finished later without reshaping this module.
//!
//! The mechanics (config-directory matrix, the `d`-cookie query, the AES-CBC + PBKDF2-SHA1
//! decrypt and its wrong-key guard, the `localConfig_v2` token layout, the keychain-account
//! ordering) are ported from gammons/slk `internal/slackdesktop` (MIT), whose cookie-decryption
//! logic is in turn adapted from rneatherway/slack (MIT). Both notices are retained in the
//! repository's `NOTICE` file.

use slakio_core::fault::{Fault, FaultKind};
use slakio_core::secret::Credentials;
use std::collections::BTreeMap;
use zeroize::Zeroizing;

#[cfg(target_os = "macos")]
mod configdir;
#[cfg(target_os = "macos")]
mod cookie;
#[cfg(target_os = "macos")]
mod decrypt;
#[cfg(target_os = "macos")]
mod keychain;
#[cfg(target_os = "macos")]
mod localstorage;
#[cfg(target_os = "macos")]
mod workspaces;

#[cfg(target_os = "macos")]
pub use keychain::SystemKeychain;

/// One signed-in workspace found in the desktop app, for the sign-in picker. No secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopWorkspace {
    /// The team id (`T…`, or `E…` for an organization): the key of its token and the secret
    /// store's account.
    pub team: String,
    /// The workspace's name, as Slack stored it (remote text: shown through the sanitiser).
    pub name: String,
    /// The workspace's host (`example.slack.com`).
    pub host: String,
}

/// The macOS "Slack Safe Storage" key source, behind a trait so the tests inject a fake and the
/// owner's real keychain is never touched by a test. Each call may raise the system's keychain
/// permission prompt.
pub trait SafeStorage: Send + Sync {
    /// Every Safe Storage key candidate for this machine, most likely first (a machine that has
    /// run more than one Slack build carries more than one). Empty is not an error here; it
    /// becomes [`DesktopError::NoSecretService`] at the point of use.
    fn keys(&self, sandboxed: bool) -> Result<Vec<Zeroizing<Vec<u8>>>, DesktopError>;
}

/// Why a desktop import cannot sign in. None of these carries any secret material or any of the
/// app's own text, so no error, log line or panic message can leak a token or a cookie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopError {
    /// Not implemented on this operating system yet (Linux, Windows): use the paste flow.
    NotSupported,
    /// The Slack desktop app's config directory was not found.
    NotFound,
    /// The app is installed but no workspace is signed in.
    NotSignedIn,
    /// The cookie database is missing (installed, never signed in).
    CookieMissing,
    /// The cookie database is locked (Slack is running and holds it): close Slack and retry.
    CookieLocked,
    /// No Safe Storage key could be read (keychain unavailable, or the prompt was declined).
    NoSecretService,
    /// The cookie could not be decrypted (wrong key, or a form this version does not read).
    DecryptFailed,
    /// A file could not be read or copied.
    Io,
}

impl DesktopError {
    /// Whether this is the ordinary "nothing to import here" outcome that should fall back to
    /// the paste flow quietly, rather than a failure worth reporting as such.
    pub fn is_absent(self) -> bool {
        matches!(
            self,
            DesktopError::NotSupported
                | DesktopError::NotFound
                | DesktopError::NotSignedIn
                | DesktopError::CookieMissing
        )
    }

    /// A [`Fault`] for the error log, with fixed words as the detail (never a secret).
    pub fn fault(self) -> Fault {
        let kind = match self {
            DesktopError::Io | DesktopError::CookieLocked | DesktopError::CookieMissing => {
                FaultKind::Io(std::io::ErrorKind::Other)
            }
            DesktopError::NoSecretService | DesktopError::DecryptFailed => FaultKind::SecretStore,
            _ => FaultKind::Other,
        };
        Fault::new(kind, self.detail())
    }

    fn detail(self) -> &'static str {
        match self {
            DesktopError::NotSupported => "desktop import is not supported on this operating system yet",
            DesktopError::NotFound => "the slack desktop app config directory was not found",
            DesktopError::NotSignedIn => "no slack workspaces are signed in to the desktop app",
            DesktopError::CookieMissing => "the slack desktop cookie database was not found",
            DesktopError::CookieLocked => "the slack desktop cookie database is locked (close slack and retry)",
            DesktopError::NoSecretService => "the safe storage key could not be read from the keychain",
            DesktopError::DecryptFailed => "the slack desktop session cookie could not be decrypted",
            DesktopError::Io => "a slack desktop file could not be read",
        }
    }
}

impl std::fmt::Display for DesktopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.detail())
    }
}

impl std::error::Error for DesktopError {}

/// What a scan of the desktop app found: the signed-in workspaces that have a session token (for
/// the picker), their tokens, and where the profile is. Reading it raises no keychain prompt; the
/// cookie (and so the prompt) is read only for the workspace the user picks, by
/// [`Found::credentials`]. The tokens are wiped from memory when this value is dropped; its
/// `Debug` print says nothing of them.
pub struct Found {
    workspaces: Vec<DesktopWorkspace>,
    /// team id -> session token (`xoxc`).
    tokens: BTreeMap<String, Zeroizing<String>>,
    dir: std::path::PathBuf,
}

impl std::fmt::Debug for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Found").field("workspaces", &self.workspaces).finish_non_exhaustive()
    }
}

impl Found {
    /// The signed-in workspaces that also have a session token, sorted by name: the only ones
    /// that can actually sign in. Shown in the picker.
    pub fn workspaces(&self) -> &[DesktopWorkspace] {
        &self.workspaces
    }

    /// The credentials for `team` using this platform's own Safe Storage (the macOS keychain,
    /// which may prompt). The cross-platform entry point for the binary; non-macOS returns
    /// [`DesktopError::NotSupported`].
    #[cfg(target_os = "macos")]
    pub fn system_credentials(&self, team: &str) -> Result<Credentials, DesktopError> {
        self.credentials(team, &SystemKeychain)
    }

    /// Non-macOS: no system Safe Storage yet.
    #[cfg(not(target_os = "macos"))]
    pub fn system_credentials(&self, _team: &str) -> Result<Credentials, DesktopError> {
        Err(DesktopError::NotSupported)
    }

    /// The credentials for `team`: its token, and the `d` (and `d-s`) cookie decrypted with the
    /// Safe Storage key from `storage` (which may raise the system's keychain prompt).
    pub fn credentials(&self, team: &str, storage: &dyn SafeStorage) -> Result<Credentials, DesktopError> {
        let token = self.tokens.get(team).ok_or(DesktopError::NotSignedIn)?;
        let (cookie, cookie_ds) = read_cookies(&self.dir, storage)?;
        let c = Credentials::new(token.as_str(), cookie.as_str());
        Ok(match cookie_ds {
            Some(ds) => c.with_ds(ds.as_str()),
            None => c,
        })
    }
}

#[cfg(target_os = "macos")]
fn read_cookies(
    dir: &std::path::Path,
    storage: &dyn SafeStorage,
) -> Result<(Zeroizing<String>, Option<Zeroizing<String>>), DesktopError> {
    cookie::cookies(dir, storage, configdir::is_sandboxed(dir))
}

#[cfg(not(target_os = "macos"))]
fn read_cookies(
    _dir: &std::path::Path,
    _storage: &dyn SafeStorage,
) -> Result<(Zeroizing<String>, Option<Zeroizing<String>>), DesktopError> {
    Err(DesktopError::NotSupported)
}

/// Scan the Slack desktop app's profile (macOS: the direct download's, else the App Store
/// build's) for workspaces that can sign in. No keychain prompt, no network. Elsewhere
/// [`DesktopError::NotSupported`].
#[cfg(target_os = "macos")]
pub fn scan() -> Result<Found, DesktopError> {
    scan_dir(&configdir::config_dir()?)
}

/// Linux and Windows: compiles, but there is no implementation yet.
#[cfg(not(target_os = "macos"))]
pub fn scan() -> Result<Found, DesktopError> {
    Err(DesktopError::NotSupported)
}

/// [`scan`] against a given profile directory (the tests' synthetic one).
#[cfg(target_os = "macos")]
pub fn scan_dir(dir: &std::path::Path) -> Result<Found, DesktopError> {
    if !dir.is_dir() {
        return Err(DesktopError::NotFound);
    }
    let found = workspaces::workspaces(dir)?;
    let mut tokens = localstorage::tokens(dir)?;
    // Keep only workspaces that have a token: the picker never offers one that cannot sign in.
    let workspaces: Vec<DesktopWorkspace> = found.into_iter().filter(|w| tokens.contains_key(&w.team)).collect();
    tokens.retain(|team, _| workspaces.iter().any(|w| &w.team == team));
    if workspaces.is_empty() {
        return Err(DesktopError::NotSignedIn);
    }
    Ok(Found { workspaces, tokens, dir: dir.to_path_buf() })
}

/// Linux and Windows: compiles, but there is no implementation yet.
#[cfg(not(target_os = "macos"))]
pub fn scan_dir(_dir: &std::path::Path) -> Result<Found, DesktopError> {
    Err(DesktopError::NotSupported)
}

/// A fresh private directory for the temp copies: created `0700` with an unpredictable name, and
/// removed with everything in it when dropped.
#[cfg(target_os = "macos")]
fn private_dir() -> Result<tempfile::TempDir, DesktopError> {
    use std::os::unix::fs::PermissionsExt;
    tempfile::Builder::new()
        .prefix("slakio-desktop-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .map_err(|_| DesktopError::Io)
}

/// The bytes of the file at `path`, refused (`FileTooLarge`) when longer than `max`, in a buffer
/// wiped when dropped (the files hold secrets). Allocated once, so no unwiped copy is left behind
/// by a reallocation.
#[cfg(target_os = "macos")]
fn read_capped(path: &std::path::Path, max: u64) -> std::io::Result<Zeroizing<Vec<u8>>> {
    use std::io::Read;
    let too_large = || std::io::Error::from(std::io::ErrorKind::FileTooLarge);
    let file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len > max {
        return Err(too_large());
    }
    let mut buf = Zeroizing::new(Vec::with_capacity(usize::try_from(len).map_err(|_| too_large())? + 1));
    file.take(max + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > max {
        return Err(too_large());
    }
    Ok(buf)
}

#[cfg(all(test, target_os = "macos"))]
mod tests;
