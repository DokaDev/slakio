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
//! `rail_expand` was a setting of the left rail, which became the top bar: a file that still has
//! it is used as it is, and the app says the key can go ([`Config::retired`]).
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

/// Set `key` to the string `value` in the config file at `path`, keeping its comments and the
/// order of what is there and the comment after the old value; a missing file is created. Used
/// for what the app saves itself (`icons` after asking, `theme`, `avatars`, `icons` and `density`
/// when changed while running).
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
