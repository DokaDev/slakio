//! Theme files: `<config dir>/themes/<name>.toml`, next to the config file.
//!
//! ```toml
//! extends = "tokyo-night"          # optional: the built-in theme it starts from
//!
//! [colors]                         # colour tokens (COLOR_TOKENS) and the four below
//! bg = "#1e1e2e"
//! warning = "yellow"               # an ANSI colour: follows the terminal's palette
//! workspaces = ["#7aa2f7", "red"]  # 1 to 16 colours, one per workspace in turn
//! avatars = ["#bb9af7", "cyan"]    # 1 to 16 colours for initials chips
//! avatar_tint = 20                 # 0 to 100: how much of a chip's colour its background keeps
//! dim = { toward = "#000000", keep = 55 }   # or "modifier"
//!
//! [styles]                         # style tokens (STYLE_TOKENS): fg, bg, modifiers
//! link = { fg = "cyan", modifiers = ["underlined"] }
//! ```
//!
//! A colour is `#rrggbb`, one of the 16 ANSI names ([`ANSI_NAMES`]) or `default` (the
//! terminal's own colour), ignoring case. A style given in the file replaces the one the UI
//! computes for that role, whole. Everything is checked strictly: an unknown key, token, colour
//! or modifier is an error with its line. Nothing here knows how a colour is drawn; the UI turns
//! a [`ThemeSpec`] into its own theme.
//!
//! A theme file is read only when it is a plain file of at most [`MAX_BYTES`]: a symbolic link
//! (the file itself; the `themes` folder may be one), a pipe or anything else is refused, and its
//! name is checked ([`valid_name`]) before a path is made of it.

use std::path::{Path, PathBuf};

mod parse;
mod words;

pub use parse::parse;

/// The largest theme file read, in bytes.
pub const MAX_BYTES: u64 = 64 * 1024;

/// The most theme files listed ([`names`]).
pub const MAX_FILES: usize = 256;
/// The most entries of a themes folder looked at for a listing ([`names`]), whatever they are:
/// a folder (a link may point anywhere) with more is listed from the first ones only.
pub const MAX_SEEN: usize = 4096;

/// The most colours of `workspaces` and of `avatars`.
pub const MAX_PALETTE: usize = 16;

/// The highest `avatar_tint` and `dim.keep` (percent).
pub const MAX_PERCENT: u16 = 100;

/// The colour tokens of `[colors]` that take one colour.
pub const COLOR_TOKENS: &[&str] = &[
    "bg",
    "surface",
    "surface_alt",
    "border",
    "accent",
    "accent_warm",
    "fg",
    "fg_muted",
    "fg_dim",
    "success",
    "warning",
    "error",
    "selection",
    "cursor_line",
    "range",
    "reaction",
    "reaction_mine",
    "mode_normal",
    "mode_insert",
    "mode_visual",
    "mode_command",
    "mode_fg",
];

/// The other tokens of `[colors]`: two lists of colours, a percent and the dimming.
pub const OTHER_COLOR_TOKENS: &[&str] = &["workspaces", "avatars", "avatar_tint", "dim"];

/// The style tokens of `[styles]`, each an override of a role the UI computes.
pub const STYLE_TOKENS: &[&str] = &[
    "selection",
    "cursor_line",
    "link",
    "mention",
    "badge",
    "current",
    "code",
    "reaction",
    "reaction_mine",
    "tab_active",
    "tab_badge",
    "status_line",
    "hint",
    "search_match",
];

/// The 16 ANSI colours in palette order (index 0 to 15).
pub const ANSI_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright-black",
    "bright-red",
    "bright-green",
    "bright-yellow",
    "bright-blue",
    "bright-magenta",
    "bright-cyan",
    "bright-white",
];

/// A colour of a theme file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpec {
    /// `default`: the terminal's own colour.
    Default,
    /// An ANSI colour by its index in [`ANSI_NAMES`].
    Ansi(u8),
    Rgb(u8, u8, u8),
}

impl ColorSpec {
    /// `#rrggbb`, an ANSI name or `default` (names ignore case).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix('#') {
            if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let v = u32::from_str_radix(hex, 16).ok()?;
            let [_, r, g, b] = v.to_be_bytes();
            return Some(ColorSpec::Rgb(r, g, b));
        }
        let lower = s.to_ascii_lowercase();
        if lower == "default" {
            return Some(ColorSpec::Default);
        }
        ANSI_NAMES.iter().position(|n| *n == lower).and_then(|i| u8::try_from(i).ok()).map(ColorSpec::Ansi)
    }
}

/// A text modifier of a style token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModifierSpec {
    Bold,
    Dim,
    Italic,
    Underlined,
    Reversed,
    CrossedOut,
}

impl ModifierSpec {
    pub const ALL: [(&'static str, ModifierSpec); 6] = [
        ("bold", ModifierSpec::Bold),
        ("dim", ModifierSpec::Dim),
        ("italic", ModifierSpec::Italic),
        ("underlined", ModifierSpec::Underlined),
        ("reversed", ModifierSpec::Reversed),
        ("crossed-out", ModifierSpec::CrossedOut),
    ];

    /// A modifier by its name (ignoring case).
    pub fn parse(s: &str) -> Option<Self> {
        let lower = s.trim().to_ascii_lowercase();
        Self::ALL.iter().find(|(n, _)| *n == lower).map(|(_, m)| *m)
    }
}

/// A style token of a theme file: what it leaves out is not set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StyleSpec {
    pub fg: Option<ColorSpec>,
    pub bg: Option<ColorSpec>,
    pub modifiers: Vec<ModifierSpec>,
}

/// How a theme dims what is behind a dialog (`dim`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DimSpec {
    /// `"modifier"`: the terminal's dim attribute, colours kept.
    Modifier,
    /// `{ toward = "#rrggbb", keep = 0 to 100 }`: every colour blended toward `toward`, keeping
    /// `keep` percent of it.
    Blend { toward: (u8, u8, u8), keep: u16 },
}

/// A theme file, read and checked. Tokens are the `&'static` names of [`COLOR_TOKENS`] and
/// [`STYLE_TOKENS`], in file order; what the file leaves out comes from the base theme.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThemeSpec {
    /// The built-in theme it starts from (lowercase), and the line that names it.
    pub extends: Option<(String, usize)>,
    pub colors: Vec<(&'static str, ColorSpec)>,
    pub workspaces: Option<Vec<ColorSpec>>,
    pub avatars: Option<Vec<ColorSpec>>,
    pub avatar_tint: Option<u16>,
    pub dim: Option<DimSpec>,
    pub styles: Vec<(&'static str, StyleSpec)>,
}

impl ThemeSpec {
    /// Whether the file names any `#rrggbb` colour (then it is a 24-bit theme).
    pub fn uses_rgb(&self) -> bool {
        let rgb = |c: &ColorSpec| matches!(c, ColorSpec::Rgb(..));
        self.colors.iter().any(|(_, c)| rgb(c))
            || self.workspaces.iter().chain(&self.avatars).flatten().any(rgb)
            || matches!(self.dim, Some(DimSpec::Blend { .. }))
            || self.styles.iter().any(|(_, s)| s.fg.iter().chain(&s.bg).any(rgb))
    }
}

/// What is wrong in a theme file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// Not TOML, an unknown key or a value of the wrong type (the parser's words).
    Toml(String),
    /// The file is not UTF-8 text.
    NotText,
    /// Not a token of any theme.
    UnknownToken(String),
    /// A style token under `[colors]`.
    NotAColor(String),
    /// A colour token under `[styles]`.
    NotAStyle(String),
    BadColor(String),
    BadModifier(String),
    /// `extends` names no built-in theme.
    UnknownBase(String),
    /// `workspaces` or `avatars` is not a list of 1 to [`MAX_PALETTE`] colours.
    BadPalette(String),
    /// `avatar_tint` is not a whole number from 0 to [`MAX_PERCENT`].
    BadPercent(String),
    /// `dim` is neither `"modifier"` nor `{ toward = "#rrggbb", keep = 0 to 100 }`.
    BadDim,
}

/// Why a theme cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThemeError {
    /// Not a name a theme file may have ([`valid_name`]); no path was made of it.
    BadName(String),
    /// No built-in theme and no theme file of this name (`dir`: where files were looked for).
    Unknown { name: String, dir: Option<PathBuf> },
    /// A theme file has the name of a built-in theme (it is never read).
    Shadows { path: PathBuf },
    /// The theme file is a symbolic link (refused, never followed).
    Symlink { path: PathBuf },
    /// The theme file is not a plain file (a pipe, a folder, a device).
    NotPlain { path: PathBuf },
    /// The theme file is larger than [`MAX_BYTES`].
    TooLarge { path: PathBuf },
    /// The file could not be read.
    Read { path: PathBuf, fault: crate::fault::Fault },
    /// The file has an error (`line`: where, when known).
    File { path: PathBuf, line: Option<usize>, problem: Problem },
}

/// Why a name cannot be given to a new theme file ([`check_name`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameError {
    /// Not a [`valid_name`], or a Windows device name.
    Invalid(String),
    /// The name of a built-in theme (ignoring case).
    Builtin(String),
    /// A word of `:theme` (`copy`, `new`, `update`, ignoring case): a file of that name could
    /// not be picked with `:theme <name>`.
    Reserved(String),
}

/// A name a theme file may have: 1 to 64 ASCII letters, digits, `-` and `_` (so it is a plain
/// file name: no `.`, `/`, `\` or NUL).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Whether `name` is the name of a built-in theme or of `auto` ([`crate::config::THEMES`]),
/// ignoring case: a theme file of that name is never read.
pub fn shadows(name: &str) -> bool {
    crate::config::THEMES.iter().any(|n| n.eq_ignore_ascii_case(name))
}

/// A name a new theme file may be given: a [`valid_name`] that is no built-in theme's, no word
/// of `:theme` (`copy`, `new`, `update`) and no Windows device name (`con`, `prn`, `aux`, `nul`, `com1` to `com9`, `lpt1` to `lpt9`),
/// ignoring case.
pub fn check_name(name: &str) -> Result<(), NameError> {
    if !valid_name(name) {
        return Err(NameError::Invalid(name.to_string()));
    }
    if shadows(name) {
        return Err(NameError::Builtin(name.to_string()));
    }
    let lower = name.to_ascii_lowercase();
    if ["copy", "new", "update"].contains(&lower.as_str()) {
        return Err(NameError::Reserved(name.to_string()));
    }
    let numbered = |prefix: &str| {
        lower.strip_prefix(prefix).is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit() && n != "0")
    };
    if ["con", "prn", "aux", "nul"].contains(&lower.as_str()) || numbered("com") || numbered("lpt") {
        return Err(NameError::Invalid(name.to_string()));
    }
    Ok(())
}

/// The theme directory of the config file `config`: `themes/` next to it.
pub fn dir(config: &Path) -> Option<PathBuf> {
    config.parent().map(|p| p.join("themes"))
}

/// The file of theme `name` in `dir`. `name` must be a [`valid_name`].
pub fn path(dir: &Path, name: &str) -> PathBuf {
    debug_assert!(valid_name(name), "a theme file's name is checked before its path is made");
    dir.join(format!("{name}.toml"))
}

/// The theme files of a folder ([`names`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// The names of the usable theme files, sorted, at most [`MAX_FILES`].
    pub names: Vec<String>,
    /// The files that have a built-in theme's name ([`ThemeError::Shadows`]), sorted.
    pub shadows: Vec<PathBuf>,
}

/// The theme files in `dir`: plain files `<name>.toml` with a [`valid_name`]. Symbolic links,
/// anything but a plain file and other names are left out without a word; a missing or
/// unreadable folder has none. `dir` itself may be a symbolic link.
/// At most [`MAX_SEEN`] entries are looked at.
pub fn names(dir: &Path) -> Listing {
    let Ok(entries) = std::fs::read_dir(dir) else { return Listing::default() };
    listing(entries.map(|e| e.ok().map(|e| e.path())))
}

/// [`names`] of the entries `paths` (`None`: one that could not be read), at most
/// [`MAX_SEEN`] of them looked at.
fn listing(paths: impl Iterator<Item = Option<PathBuf>>) -> Listing {
    let mut out = Listing::default();
    for path in paths.take(MAX_SEEN).flatten() {
        if path.extension().and_then(|x| x.to_str()) != Some("toml") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()).filter(|s| valid_name(s)) else { continue };
        // Not followed: a link is not a plain file here.
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file()) {
            continue;
        }
        if shadows(stem) {
            out.shadows.push(path.clone());
        } else {
            out.names.push(stem.to_string());
        }
    }
    out.names.sort();
    out.names.truncate(MAX_FILES);
    out.shadows.sort();
    out
}

/// Read and check theme `name` from `dir`. The name is checked first; a file that is not there
/// is [`ThemeError::Unknown`]. Nothing is created.
pub fn read(dir: &Path, name: &str) -> Result<ThemeSpec, ThemeError> {
    read_text(dir, name).map(|(_, spec)| spec)
}

/// [`read`], with the text of the file as it was read.
pub fn read_text(dir: &Path, name: &str) -> Result<(String, ThemeSpec), ThemeError> {
    if !valid_name(name) {
        return Err(ThemeError::BadName(name.to_string()));
    }
    let path = path(dir, name);
    if shadows(name) {
        return Err(ThemeError::Shadows { path });
    }
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ThemeError::Unknown { name: name.to_string(), dir: Some(dir.to_path_buf()) });
        }
        Err(e) => return Err(ThemeError::Read { fault: crate::fault::Fault::io_at(&e, &path), path }),
    };
    if meta.file_type().is_symlink() {
        return Err(ThemeError::Symlink { path });
    }
    if !meta.file_type().is_file() {
        return Err(ThemeError::NotPlain { path });
    }
    if meta.len() > MAX_BYTES {
        return Err(ThemeError::TooLarge { path });
    }
    // Opened without following a link or waiting on a pipe, in case the file changed since.
    let bytes = match crate::fsutil::read_plain(&path, MAX_BYTES) {
        Ok(b) => b,
        Err(e) => return Err(ThemeError::Read { fault: crate::fault::Fault::io_at(&e, &path), path }),
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return Err(ThemeError::File { path, line: None, problem: Problem::NotText });
    };
    match parse(&text) {
        Ok(spec) => Ok((text, spec)),
        Err((line, problem)) => Err(ThemeError::File { path, line, problem }),
    }
}

#[cfg(test)]
mod tests;
