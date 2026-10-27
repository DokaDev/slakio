//! The config file, `config.toml` in the config directory ([`crate::paths::Paths::config_file`])
//! or the file given with `--config`.
//!
//! ```toml
//! language = "auto"        # "auto" (from LC_ALL / LC_MESSAGES / LANG), "en" or "ko"
//! theme = "auto"           # "auto", "terminal", "dark", "light", "high-contrast", "nord",
//!                          # "dracula", or a family: "catppuccin" (-latte / -mocha),
//!                          # "tokyo-night" (-day / -night), "gruvbox" (-light / -dark)
//! icons = "ask"            # Nerd Font icons: "on", "off", or "ask" once (true/false work too)
//! avatars = "initials"     # a person's initials on a colored chip, or "off"
//! density = "comfortable"  # messages as in GUI Slack (avatar block, name over the text), or "compact"
//! nav_rows = "expanded"    # the view switcher a row per view, or "collapsed" to the view shown
//! network = "on"           # "off": nothing is sent to Slack at all (also SLAKIO_NETWORK=off)
//! cache = "on"             # "off": nothing is kept on disk between runs (every start fetches)
//! cache_messages = 200     # the most messages kept a conversation (10 to 10000)
//! cache_max_mb = 500       # the most megabytes the cache may use (10 to 100000)
//! cache_days = 30          # a conversation not opened for this many days is dropped (1 to 3650)
//! ```
//!
//! `theme = "auto"` takes `tokyo-night` on a terminal that says it shows 24-bit color, else the
//! terminal's own colors. A family takes its light or its dark variant by the terminal's
//! background. `:theme <name>` changes it while the app runs and saves it here (comments kept).
//!
//! `avatars = "image"` is kept for profile photos, which come in a later version; until then it
//! draws initials, as it will where a terminal cannot show images. `:avatars` changes the
//! setting while the app runs and saves it here too.
//!
//! `rail_expand` was a setting of the left rail (later the top bar, now the view switcher): a
//! file that still has it is used as it is, and the app says the key can go
//! ([`Config::retired`]).
//!
//! A missing file is the defaults. A file that exists but cannot be used (unreadable, not valid
//! TOML, an unknown key, a value out of range) is never treated as missing: the app runs with
//! the defaults, says so, and never writes over it. The UI words each [`ConfigError`]; the raw
//! detail goes to the error log.

use crate::fault::Fault;
use std::path::{Path, PathBuf};

/// The settings, with their defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// The UI language setting: `auto`, `en` or `ko` ([`crate::i18n::detect_lang`]).
    pub language: String,
    /// The color theme: `auto` or a built-in ([`THEMES`]).
    pub theme: String,
    /// Nerd Font icons instead of letters: `on`, `off`, or `ask` (asked once, the answer saved).
    pub icons: String,
    /// How a person is pictured: `initials`, `off`, or `image` (initials for now) ([`AVATARS`]).
    pub avatars: String,
    /// How messages are laid out: `comfortable` or `compact` ([`DENSITIES`]).
    pub density: String,
    /// The view switcher a row per view (`expanded`) or folded to the view shown
    /// (`collapsed`) ([`NAV_ROWS`]).
    pub nav_rows: String,
    /// `off`: no request leaves the process ([`NETWORK`]); the environment can turn it off too.
    pub network: String,
    /// `off`: nothing is read from or kept in the local cache ([`CACHE`]).
    pub cache: String,
    /// The most messages the cache keeps of a conversation ([`CACHE_MESSAGES`]).
    pub cache_messages: u32,
    /// The most megabytes the cache may use ([`CACHE_MAX_MB`]).
    pub cache_max_mb: u32,
    /// A conversation not opened for this many days leaves the cache ([`CACHE_DAYS`]).
    pub cache_days: u32,
    /// The file the settings came from (or would be written to), when known.
    pub path: Option<PathBuf>,
    /// Keys of an older version the file still has, ignored ([`RETIRED`]): the app says they
    /// can go, and uses the file.
    pub retired: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: "auto".to_string(),
            theme: "auto".to_string(),
            icons: "ask".to_string(),
            avatars: "initials".to_string(),
            density: "comfortable".to_string(),
            nav_rows: "expanded".to_string(),
            network: "on".to_string(),
            cache: "on".to_string(),
            cache_messages: 200,
            cache_max_mb: 500,
            cache_days: 30,
            path: None,
            retired: Vec::new(),
        }
    }
}

/// The values `language` takes.
pub const LANGUAGES: &[&str] = &["auto", "en", "ko"];

/// The values `theme` takes (the UI's built-in themes).
pub const THEMES: &[&str] = &[
    "auto",
    "terminal",
    "dark",
    "light",
    "high-contrast",
    "catppuccin",
    "catppuccin-latte",
    "catppuccin-mocha",
    "tokyo-night",
    "tokyo-night-day",
    "tokyo-night-night",
    "gruvbox",
    "gruvbox-light",
    "gruvbox-dark",
    "nord",
    "dracula",
];

/// The values `icons` takes (`true` and `false` are `on` and `off`).
pub const ICONS: &[&str] = &["on", "off", "ask"];

/// The values `avatars` takes.
pub const AVATARS: &[&str] = &["initials", "off", "image"];

/// The values `density` takes.
pub const DENSITIES: &[&str] = &["comfortable", "compact"];

/// The values `nav_rows` takes.
pub const NAV_ROWS: &[&str] = &["expanded", "collapsed"];

/// The values `network` takes.
pub const NETWORK: &[&str] = &["on", "off"];

/// The values `cache` takes.
pub const CACHE: &[&str] = &["on", "off"];

/// The range of `cache_messages`.
pub const CACHE_MESSAGES: std::ops::RangeInclusive<u32> = 10..=10_000;

/// The range of `cache_max_mb`.
pub const CACHE_MAX_MB: std::ops::RangeInclusive<u32> = 10..=100_000;

/// The range of `cache_days`.
pub const CACHE_DAYS: std::ops::RangeInclusive<u32> = 1..=3650;

/// Keys an older version had: ignored, never an error ([`Config::retired`]).
pub const RETIRED: &[&str] = &["rail_expand"];

/// Why the config file cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// The file exists but could not be read.
    Read { path: PathBuf, fault: Fault },
    /// It is not valid TOML.
    Syntax(Fault),
    /// A key this version does not know.
    UnknownKey(String),
    /// `key = value` is not one of `allowed`.
    Value { key: String, value: String, allowed: String },
}

impl ConfigError {
    /// The raw detail for the error log.
    pub fn fault(&self) -> Fault {
        match self {
            ConfigError::Read { fault, .. } | ConfigError::Syntax(fault) => fault.clone(),
            ConfigError::UnknownKey(k) => Fault::other(format!("unknown key {k}")),
            ConfigError::Value { key, value, .. } => Fault::other(format!("{key} = {value}")),
        }
    }
}

/// Load the config file at `path` (`None`: no config directory, the defaults).
pub fn load(path: Option<&Path>) -> (Config, Option<ConfigError>) {
    let defaults = Config { path: path.map(Path::to_path_buf), ..Config::default() };
    let Some(path) = path else { return (defaults, None) };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (defaults, None),
        Err(e) => {
            return (defaults, Some(ConfigError::Read { path: path.to_path_buf(), fault: Fault::io_at(&e, path) }));
        }
    };
    match parse(&text) {
        Ok(cfg) => (Config { path: defaults.path.clone(), ..cfg }, None),
        Err(e) => (defaults, Some(e)),
    }
}

/// The settings of a config file's text.
fn parse(text: &str) -> Result<Config, ConfigError> {
    let doc = text.parse::<toml_edit::DocumentMut>().map_err(|e| ConfigError::Syntax(Fault::toml_edit(text, &e)))?;
    let mut cfg = Config::default();
    for (key, item) in doc.iter() {
        let bad = |allowed: String| ConfigError::Value {
            key: key.to_string(),
            value: item.to_string().trim().to_string(),
            allowed,
        };
        match key {
            "language" => cfg.language = one_of(item, LANGUAGES).ok_or_else(|| bad(LANGUAGES.join(", ")))?,
            k if RETIRED.contains(&k) => cfg.retired.push(k.to_string()),
            "avatars" => cfg.avatars = one_of(item, AVATARS).ok_or_else(|| bad(AVATARS.join(", ")))?,
            "density" => cfg.density = one_of(item, DENSITIES).ok_or_else(|| bad(DENSITIES.join(", ")))?,
            "nav_rows" => cfg.nav_rows = one_of(item, NAV_ROWS).ok_or_else(|| bad(NAV_ROWS.join(", ")))?,
            "network" => cfg.network = one_of(item, NETWORK).ok_or_else(|| bad(NETWORK.join(", ")))?,
            "cache" => cfg.cache = one_of(item, CACHE).ok_or_else(|| bad(CACHE.join(", ")))?,
            "cache_messages" => {
                cfg.cache_messages = within(item, &CACHE_MESSAGES).ok_or_else(|| bad(span(&CACHE_MESSAGES)))?
            }
            "cache_max_mb" => cfg.cache_max_mb = within(item, &CACHE_MAX_MB).ok_or_else(|| bad(span(&CACHE_MAX_MB)))?,
            "cache_days" => cfg.cache_days = within(item, &CACHE_DAYS).ok_or_else(|| bad(span(&CACHE_DAYS)))?,
            "theme" => cfg.theme = one_of(item, THEMES).ok_or_else(|| bad(THEMES.join(", ")))?,
            "icons" => {
                cfg.icons = match item.as_bool() {
                    Some(true) => "on".to_string(),
                    Some(false) => "off".to_string(),
                    None => one_of(item, ICONS).ok_or_else(|| bad(ICONS.join(", ")))?,
                }
            }
            other => return Err(ConfigError::UnknownKey(other.to_string())),
        }
    }
    Ok(cfg)
}

/// The string value of `item` when it is one of `allowed` (case ignored), lowercased.
fn one_of(item: &toml_edit::Item, allowed: &[&str]) -> Option<String> {
    item.as_str().map(str::to_ascii_lowercase).filter(|v| allowed.contains(&v.as_str()))
}

/// The whole number `item` when it is in `range`.
fn within(item: &toml_edit::Item, range: &std::ops::RangeInclusive<u32>) -> Option<u32> {
    item.as_integer().and_then(|n| u32::try_from(n).ok()).filter(|n| range.contains(n))
}

/// The words for a range of whole numbers (`10 to 10000`).
fn span(range: &std::ops::RangeInclusive<u32>) -> String {
    format!("a whole number from {} to {}", range.start(), range.end())
}

/// Set `key` to the string `value` in the config file at `path`, keeping its comments and the
/// order of what is there and the comment after the old value; a missing file is created. Used
/// for what the app saves itself (`icons` after asking, `theme`, `avatars`, `icons`, `density`
/// and `nav_rows` when changed while running).
/// A file that cannot be read or parsed is left alone.
pub fn set(path: &Path, key: &str, value: &str) -> Result<(), Fault> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Fault::io_at(&e, path)),
    };
    let mut doc = text.parse::<toml_edit::DocumentMut>().map_err(|e| Fault::toml_edit(&text, &e))?;
    let mut item = toml_edit::value(value);
    // Keep what surrounds the old value, such as a comment after it on the same line.
    if let (Some(old), Some(new)) = (doc.get(key).and_then(toml_edit::Item::as_value), item.as_value_mut()) {
        *new.decor_mut() = old.decor().clone();
    }
    doc[key] = item;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Fault::io_at(&e, dir))?;
    }
    crate::fsutil::atomic_write(path, doc.to_string().as_bytes()).map_err(|e| Fault::io_at(&e, path))
}

#[cfg(test)]
mod tests;
