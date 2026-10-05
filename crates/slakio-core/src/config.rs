//! The config file, `config.toml` in the config directory ([`crate::paths::Paths::config_file`])
//! or the file given with `--config`.
//!
//! ```toml
//! language = "auto"   # "auto" (from LC_ALL / LC_MESSAGES / LANG), "en" or "ko"
//! ```
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
    /// The file the settings came from (or would be written to), when known.
    pub path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self { language: "auto".to_string(), path: None }
    }
}

/// The values `language` takes.
pub const LANGUAGES: &[&str] = &["auto", "en", "ko"];

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
        Ok(language) => (Config { language, ..defaults }, None),
        Err(e) => (defaults, Some(e)),
    }
}

/// The settings of a config file's text.
fn parse(text: &str) -> Result<String, ConfigError> {
    let doc = text.parse::<toml_edit::DocumentMut>().map_err(|e| ConfigError::Syntax(Fault::toml_edit(text, &e)))?;
    let mut language = Config::default().language;
    for (key, item) in doc.iter() {
        match key {
            "language" => {
                let value = item.as_str().map(str::to_ascii_lowercase);
                match value {
                    Some(v) if LANGUAGES.contains(&v.as_str()) => language = v,
                    _ => {
                        return Err(ConfigError::Value {
                            key: key.to_string(),
                            value: item.to_string().trim().to_string(),
                            allowed: LANGUAGES.join(", "),
                        });
                    }
                }
            }
            other => return Err(ConfigError::UnknownKey(other.to_string())),
        }
    }
    Ok(language)
}

#[cfg(test)]
mod tests;
