//! Where slakio keeps its files: the config directory (the config file), the data directory
//! (what the user would miss: drafts), the state directory (one-time notices, the error log,
//! the instance lock) and the cache directory (anything that can be fetched again).
//!
//! Precedence, for each directory: the `SLAKIO_CONFIG_DIR` / `SLAKIO_DATA_DIR` /
//! `SLAKIO_STATE_DIR` / `SLAKIO_CACHE_DIR` override, then `$XDG_CONFIG_HOME/slakio` /
//! `$XDG_DATA_HOME/slakio` / `$XDG_STATE_HOME/slakio` / `$XDG_CACHE_HOME/slakio` when set (on
//! every OS, so a development run pointed at scratch XDG directories never touches the real
//! ones), then the platform default:
//!
//! | | Linux and other Unix | macOS | Windows |
//! |---|---|---|---|
//! | config | `~/.config/slakio` | `~/.config/slakio` | `%USERPROFILE%\.config\slakio` |
//! | data | `~/.local/share/slakio` | `~/Library/Application Support/slakio` | `%APPDATA%\slakio` |
//! | state | `~/.local/state/slakio` | `~/Library/Application Support/slakio/state` | `%LOCALAPPDATA%\slakio` |
//! | cache | `~/.cache/slakio` | `~/Library/Caches/slakio` | `%LOCALAPPDATA%\slakio\cache` |

use std::path::{Path, PathBuf};

/// The platform whose default directories apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Unix,
    MacOs,
    Windows,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Unix
        }
    }
}

/// The app's directories. `None` when neither an override nor a home directory is known
/// (nothing is read or written there then).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Paths {
    pub config: Option<PathBuf>,
    pub data: Option<PathBuf>,
    pub state: Option<PathBuf>,
    pub cache: Option<PathBuf>,
}

impl Paths {
    /// From the process environment.
    pub fn from_env() -> Self {
        Self::resolve(|k| std::env::var(k).ok(), Os::current())
    }

    /// From `env` on `os` (tests pass both).
    pub fn resolve(env: impl Fn(&str) -> Option<String>, os: Os) -> Self {
        let var = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = || var("HOME").or_else(|| var("USERPROFILE"));
        let xdg = |k: &str| var(k).map(|d| d.join("slakio"));
        let config = var("SLAKIO_CONFIG_DIR")
            .or_else(|| xdg("XDG_CONFIG_HOME"))
            .or_else(|| home().map(|h| h.join(".config").join("slakio")));
        let data = var("SLAKIO_DATA_DIR").or_else(|| xdg("XDG_DATA_HOME")).or_else(|| match os {
            Os::Unix => home().map(|h| h.join(".local").join("share").join("slakio")),
            Os::MacOs => home().map(|h| mac_support(&h)),
            Os::Windows => var("APPDATA").map(|d| d.join("slakio")),
        });
        let state = var("SLAKIO_STATE_DIR").or_else(|| xdg("XDG_STATE_HOME")).or_else(|| match os {
            Os::Unix => home().map(|h| h.join(".local").join("state").join("slakio")),
            Os::MacOs => home().map(|h| mac_support(&h).join("state")),
            Os::Windows => var("LOCALAPPDATA").map(|d| d.join("slakio")),
        });
        let cache = var("SLAKIO_CACHE_DIR").or_else(|| xdg("XDG_CACHE_HOME")).or_else(|| match os {
            Os::Unix => home().map(|h| h.join(".cache").join("slakio")),
            Os::MacOs => home().map(|h| h.join("Library").join("Caches").join("slakio")),
            Os::Windows => var("LOCALAPPDATA").map(|d| d.join("slakio").join("cache")),
        });
        Paths { config, data, state, cache }
    }

    /// `<config>/config.toml`.
    pub fn config_file(&self) -> Option<PathBuf> {
        self.config.as_ref().map(|c| c.join("config.toml"))
    }

    /// `<state>/notices.toml`: notices that are shown once per machine.
    pub fn notices_file(&self) -> Option<PathBuf> {
        self.state.as_ref().map(|s| s.join("notices.toml"))
    }

    /// `<state>/errors.log`: the raw detail of failures the UI reported in words
    /// ([`crate::fault::ErrorLog`]).
    pub fn errors_log(&self) -> Option<PathBuf> {
        self.state.as_ref().map(|s| s.join("errors.log"))
    }

    /// Whether the one-time notice `key` was not shown on this machine yet; records that it
    /// is now. Without a state directory every run counts as the first. A file that cannot be
    /// read or parsed is never written over: the notice shows, and is not recorded.
    pub fn first_time(&self, key: &str) -> bool {
        let Some(file) = self.notices_file() else { return true };
        let text = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(_) => return true,
        };
        let Ok(mut doc) = text.parse::<toml_edit::DocumentMut>() else { return true };
        if doc.get(key).and_then(toml_edit::Item::as_bool) == Some(true) {
            return false;
        }
        doc[key] = toml_edit::value(true);
        let _ = crate::fsutil::atomic_write(&file, doc.to_string().as_bytes());
        true
    }
}

fn mac_support(home: &Path) -> PathBuf {
    home.join("Library").join("Application Support").join("slakio")
}

#[cfg(test)]
mod tests;
