//! The Slack desktop app's config directory. Ported from gammons/slk
//! `internal/slackdesktop/configdir.go` (MIT). macOS has two locations: the direct download's
//! `~/Library/Application Support/Slack` and the App Store (sandboxed) build's container under
//! `~/Library/Containers/com.tinyspeck.slackmacgap`. The first existing one wins.
//!
//! The Linux and Windows candidate lists are kept here as data (not reachable yet) so the port
//! can be finished without reshaping the module; only the macOS branch is compiled and called.

use super::DesktopError;
use std::path::{Path, PathBuf};

/// Compute the config directory for `goos`, with `home` and `exists` injected so the logic is
/// tested without touching the real filesystem. Returns the first candidate that exists, else
/// the conventional location (so the not-found error names it).
pub(super) fn config_dir_for(
    goos: &str,
    getenv: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&Path) -> bool,
) -> PathBuf {
    match goos {
        "macos" => {
            let home = PathBuf::from(getenv("HOME").unwrap_or_default());
            let direct = home.join("Library/Application Support/Slack");
            let app_store =
                home.join("Library/Containers/com.tinyspeck.slackmacgap/Data/Library/Application Support/Slack");
            if exists(&direct) { direct } else { app_store }
        }
        "windows" => {
            let mut candidates = Vec::new();
            if let Some(appdata) = getenv("APPDATA") {
                candidates.push(PathBuf::from(appdata).join("Slack"));
            }
            if let Some(local) = getenv("LOCALAPPDATA") {
                candidates.push(
                    PathBuf::from(local)
                        .join("Packages/com.tinyspeck.slackdesktop_8yrtsj140pw4g/LocalCache/Roaming/Slack"),
                );
            }
            first_existing(candidates, exists)
        }
        _ => {
            let home = PathBuf::from(getenv("HOME").unwrap_or_default());
            let mut candidates = Vec::new();
            if let Some(x) = getenv("XDG_CONFIG_HOME") {
                candidates.push(PathBuf::from(x).join("Slack"));
            }
            if let Some(x) = getenv("XDG_CONFIG_DIR") {
                candidates.push(PathBuf::from(x).join("Slack"));
            }
            candidates.push(home.join(".config/Slack"));
            candidates.push(home.join(".var/app/com.slack.Slack/config/Slack"));
            candidates.push(home.join("snap/slack/current/.config/Slack"));
            first_existing(candidates, exists)
        }
    }
}

fn first_existing(candidates: Vec<PathBuf>, exists: &dyn Fn(&Path) -> bool) -> PathBuf {
    for c in &candidates {
        if exists(c) {
            return c.clone();
        }
    }
    candidates.into_iter().next().unwrap_or_default()
}

/// The real config directory, or [`DesktopError::NotFound`] when it is not a directory on disk.
pub(super) fn config_dir() -> Result<PathBuf, DesktopError> {
    let dir = config_dir_for("macos", &|k| std::env::var(k).ok(), &|p| p.is_dir());
    if dir.is_dir() { Ok(dir) } else { Err(DesktopError::NotFound) }
}

/// Whether `dir` is the App Store (sandboxed) build's container. The matching keychain account
/// is tried first when it is (keychain-account ordering, see `keychain`).
pub(super) fn is_sandboxed(dir: &Path) -> bool {
    dir.to_string_lossy().contains("/Library/Containers/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| pairs.iter().find(|(key, _)| *key == k).map(|(_, v)| (*v).to_string())
    }

    #[test]
    fn macos_prefers_direct_download() {
        let home = [("HOME", "/home/u")];
        let direct = PathBuf::from("/home/u/Library/Application Support/Slack");
        let dir = config_dir_for("macos", &env(&home), &|p| p == direct);
        assert_eq!(dir, direct);
        assert!(!is_sandboxed(&dir));
    }

    #[test]
    fn macos_falls_back_to_app_store_container() {
        let home = [("HOME", "/home/u")];
        let dir = config_dir_for("macos", &env(&home), &|_| false);
        assert!(is_sandboxed(&dir), "{dir:?}");
    }

    #[test]
    fn linux_honours_xdg_then_conventional() {
        let pairs = [("HOME", "/home/u"), ("XDG_CONFIG_HOME", "/cfg")];
        let xdg = PathBuf::from("/cfg/Slack");
        assert_eq!(config_dir_for("linux", &env(&pairs), &|p| p == xdg), xdg);
        // Nothing exists: the first candidate (the XDG one) is named.
        assert_eq!(config_dir_for("linux", &env(&pairs), &|_| false), xdg);
    }

    #[test]
    fn windows_uses_appdata_first() {
        let pairs = [("APPDATA", "C:/AppData"), ("LOCALAPPDATA", "C:/Local")];
        let appdata = PathBuf::from("C:/AppData").join("Slack");
        assert_eq!(config_dir_for("windows", &env(&pairs), &|_| false), appdata);
    }
}
