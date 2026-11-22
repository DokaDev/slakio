//! Writing theme files. `:theme copy` writes every colour token of a theme set, so the file
//! stands on its own, and every style token under it: set where the theme sets it, else
//! commented out with the look it takes now (it follows the colours). `:theme new` writes every
//! token of the built-in theme it extends, commented out, to be filled in. Each key has what it
//! is for on the line above ([`COLOR_ABOUT`], [`STYLE_ABOUT`]), the words of `docs/themes.md`.
//!
//! Those make a new file, never replace one: a name that is taken is [`AuthorError::Exists`],
//! whatever the file holds. The `themes` folder is made here only (0700), the files are 0600.
//! `:theme update` is in [`update`].

mod update;

pub use update::{Updated, update};

use super::custom::{ANSI, from_spec};
use super::{Dim, Look, NAMES, Theme};
use ratatui::style::{Color, Modifier, Style};
use slakio_core::fsutil;
use slakio_core::theme_file::{self, ANSI_NAMES, MAX_PALETTE, ModifierSpec, NameError, ThemeError};
use std::path::{Path, PathBuf};

/// Why a theme file was not written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthorError {
    /// Not a name a new theme file may have (checked before any path is made of it).
    Name(NameError),
    /// The theme to copy (or to update) cannot be used.
    Source(ThemeError),
    /// A token has a value a theme file cannot hold.
    Unwritable { token: &'static str },
    /// There is a file of that name already: it is left as it is.
    Exists(PathBuf),
    /// The file (or the folder) could not be written.
    Write(PathBuf),
    /// The themes folder is not the user's own, or others may write in it: nothing is written
    /// there (reading from it stays allowed).
    Shared(PathBuf),
    /// A built-in theme: there is no file to update.
    NotAFile(String),
    /// The file changed after it was read, or would not read as it did: it is left as it is.
    Changed(PathBuf),
}

/// What each colour token colours (the 20 single colours, then slakio's four), as
/// `docs/themes.md` says it.
pub const COLOR_ABOUT: &[(&str, &str)] = &[
    ("bg", "The app's background."),
    ("surface", "The status line and popups."),
    ("surface_alt", "Reaction pills."),
    ("border", "Borders of panels without the focus, dividers."),
    ("accent", "The focused panel's border, links, prompts, the current workspace, keys in popups."),
    ("accent_warm", "Key groups, a mention of you, words a search found."),
    ("fg", "Body text."),
    ("fg_muted", "Section headers, titles without the focus, hint labels."),
    ("fg_dim", "Times, date lines, muted conversations, placeholders."),
    ("success", "Success, and the mark of someone active."),
    ("warning", "Warnings, the mark of do not disturb, the letters that pick a link."),
    ("error", "Errors, and the background of the unread and mention counts."),
    ("selection", "The background of the selected row of the focused panel."),
    ("cursor_line", "The background of the selected row of a panel without the focus (24-bit themes)."),
    ("range", "The background of a VISUAL range."),
    ("mode_normal", "The mode badge in NORMAL."),
    ("mode_insert", "The mode badge in INSERT."),
    ("mode_visual", "The mode badge in VISUAL."),
    ("mode_command", "The mode badge while the command palette is open."),
    ("mode_fg", "The text of the mode badge and of the counts."),
    (
        "workspaces",
        "A list of 1 to 16 colours, taken by workspaces in sign-in order (wrapping round). A colour too close to \
         `accent` is skipped, so a workspace's stripe never reads as a focus frame.",
    ),
    ("avatars", "A list of 1 to 16 colours for the initials chips of people."),
    (
        "avatar_tint",
        "A whole number from 0 to 100: the percent of a person's colour mixed into the background for their chip. \
         `0` and `100` draw the colour solid under `mode_fg`. It only changes the initials chips, never pictures.",
    ),
    (
        "dim",
        "How the screen behind a dialog or the palette is dimmed: `\"modifier\"` (the terminal's dim attribute, \
         colours kept), or `{ toward = \"#rrggbb\", keep = 0 to 100 }`: every colour blended toward `toward`, \
         keeping `keep` percent of it. `toward` must be `#rrggbb`.",
    ),
];

/// What each style token styles, as `docs/themes.md` says it.
pub const STYLE_ABOUT: &[(&str, &str)] = &[
    (
        "selection",
        "The selected row of the focused panel. Its `bg` replaces the bar's colour; its `fg` and modifiers go on the \
         row's text.",
    ),
    ("cursor_line", "The selected row of a panel without the focus, as `selection`."),
    ("link", "Links in a message, and the \"N replies\" row of a message with a thread."),
    ("mention", "A person, channel or group named in a message (also a mention of you)."),
    ("badge", "The count of mentions and unread DMs."),
    ("current", "The current workspace or view."),
    ("code", "Code in a message, inline or a block's lines."),
    ("reaction", "A reaction chip."),
    ("reaction_mine", "Your own reaction."),
    ("tab_active", "The tab shown: its title, number and close button."),
    ("tab_badge", "A tab's unread count."),
    ("status_line", "The status line."),
    ("hint", "The letters that pick a link (`o` on a message with several)."),
    ("search_match", "A word a search found, in a result's text."),
];

/// The first name of `<from>-copy`, `<from>-copy-2`, … with no file in `dir`: always a valid
/// name (`from`'s letters a name can have, cut so the longest suffix still fits; `theme` when
/// none is left).
pub fn free_copy_name(dir: Option<&Path>, from: &str) -> String {
    let taken = |name: &str| {
        dir.is_some_and(|d| theme_file::valid_name(name) && theme_file::path(d, name).symlink_metadata().is_ok())
    };
    let stem: String = from
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64 - "-copy-999".len())
        .collect();
    let stem = if stem.is_empty() { "theme" } else { &stem };
    let first = format!("{stem}-copy");
    if !taken(&first) {
        return first;
    }
    (2..1000).map(|n| format!("{first}-{n}")).find(|n| !taken(n)).unwrap_or(first)
}

/// `:theme copy <from> <name>`: write `<dir>/<name>.toml` with every token of theme `from` (a
/// built-in theme, `auto` or a theme file) as it is drawn on `look` now, extending the built-in
/// theme it starts from. Hands back the file's path.
pub fn copy(dir: &Path, from: &str, name: &str, look: Look) -> Result<PathBuf, AuthorError> {
    theme_file::check_name(name).map_err(AuthorError::Name)?;
    let look = Look { no_color: false, ..look };
    let lower = from.to_ascii_lowercase();
    let (th, base) = if NAMES.contains(&lower.as_str()) {
        let th = look.theme(&lower);
        let base = th.name.to_string();
        (th, base)
    } else {
        if !theme_file::valid_name(from) {
            return Err(AuthorError::Source(ThemeError::BadName(from.to_string())));
        }
        let spec = theme_file::read(dir, from).map_err(AuthorError::Source)?;
        let base = spec.extends.as_ref().map_or("terminal", |(b, _)| b.as_str());
        let base = super::resolve(base, look.truecolor, look.background).name.to_string();
        (from_spec(from, &spec, look), base)
    };
    let header = format!(
        "# A slakio theme file: a copy of \"{from}\" with every colour token set, so it stands on its own.\n\
         # A style it does not set is listed commented out with the look it takes now: it follows\n\
         # the colours. Remove the \"# \" in front of a key to set it.\n"
    );
    write(dir, name, &file_text(&header, &base, &th, false)?)
}

/// `:theme new <name>`: write `<dir>/<name>.toml` extending the built-in theme `base`, every
/// token commented out with the value it takes from `base` on `look` now. Hands back the path.
pub fn new(dir: &Path, name: &str, base: &str, look: Look) -> Result<PathBuf, AuthorError> {
    theme_file::check_name(name).map_err(AuthorError::Name)?;
    let base = base.to_ascii_lowercase();
    if base == "auto" || !NAMES.contains(&base.as_str()) {
        return Err(AuthorError::Source(ThemeError::Unknown { name: base, dir: None }));
    }
    let th = Look { no_color: false, ..look }.theme(&base);
    let header = format!(
        "# A slakio theme file extending \"{base}\". Every token is listed with the value it takes\n\
         # from \"{base}\" (as drawn now), commented out: remove the \"# \" in front of a key to set it.\n"
    );
    write(dir, name, &file_text(&header, &base, &th, true)?)
}

/// Make `<dir>/<name>.toml` (and `dir`, only its owner's, when it is missing) with `text`.
fn write(dir: &Path, name: &str, text: &str) -> Result<PathBuf, AuthorError> {
    shared_check(dir)?;
    let path = theme_file::path(dir, name);
    match fsutil::create_new_private(&path, text.as_bytes()) {
        Ok(()) => Ok(path),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(AuthorError::Exists(path)),
        Err(_) => Err(AuthorError::Write(path)),
    }
}

/// The themes folder `dir` made (only its owner's) when missing, and refused when it is shared:
/// someone else's, or one others may write in (a link's target is never changed). Apart from
/// the file, so a folder that cannot be made is never taken for a taken name.
fn shared_check(dir: &Path) -> Result<(), AuthorError> {
    match fsutil::own_private_dir(dir) {
        Ok(true) => Ok(()),
        Ok(false) => Err(AuthorError::Shared(dir.to_path_buf())),
        Err(_) => Err(AuthorError::Write(dir.to_path_buf())),
    }
}

/// The lines every written file explains the values with (the words of `docs/themes.md`).
const FORMAT: &str = "\
# A colour is #rrggbb, an ANSI name (black, red, green, yellow, blue, magenta, cyan, white, and each
# with bright- in front) or default (the terminal's own). A style is { fg = <colour>,
# bg = <colour>, modifiers = [...] }, each part optional; what it leaves out is not set.
# Modifiers: bold, dim, italic, underlined, reversed, crossed-out.
# The format and every token: docs/themes.md in slakio's repository.
";

/// The text of a theme file: `header`, `extends = "<base>"` and every token of `th` under what
/// it is for, commented out when `commented` (a style `th` does not set always is).
fn file_text(header: &str, base: &str, th: &Theme, commented: bool) -> Result<String, AuthorError> {
    let mut out = format!("{header}{FORMAT}extends = \"{base}\"\n\n[colors]\n");
    let off = if commented { "# " } else { "" };
    for (token, about) in COLOR_ABOUT {
        let value = color_value(th, token).ok_or(AuthorError::Unwritable { token })?;
        out.push_str(&format!("\n# {}\n{off}{token} = {value}\n", about.replace('`', "")));
    }
    out.push_str("\n[styles]\n");
    for (token, about) in STYLE_ABOUT {
        let (value, set) = style_value(th, token).ok_or(AuthorError::Unwritable { token })?;
        let off = if set { off } else { "# " };
        out.push_str(&format!("\n# {}\n{off}{token} = {value}\n", about.replace('`', "")));
    }
    Ok(out)
}

/// Colour token `token` of `th` as a theme file writes it; `None`: one it cannot hold.
pub(super) fn color_value(th: &Theme, token: &str) -> Option<String> {
    let list = |colors: &[Color]| -> Option<String> {
        if colors.is_empty() || colors.len() > MAX_PALETTE {
            return None;
        }
        let shown: Option<Vec<String>> = colors.iter().map(|c| color_text(*c)).collect();
        Some(format!("[{}]", shown?.join(", ")))
    };
    let c = match token {
        "workspaces" => return list(&th.workspaces),
        "avatars" => return list(&th.avatars),
        "avatar_tint" => return (th.avatar_tint <= theme_file::MAX_PERCENT).then(|| th.avatar_tint.to_string()),
        "dim" => {
            return match th.dim {
                Dim::Modifier => Some("\"modifier\"".to_string()),
                Dim::Blend { toward: Color::Rgb(r, g, b), keep } if keep <= theme_file::MAX_PERCENT => {
                    Some(format!("{{ toward = \"#{r:02x}{g:02x}{b:02x}\", keep = {keep} }}"))
                }
                Dim::Blend { .. } => None,
            };
        }
        "bg" => th.bg,
        "surface" => th.surface,
        "surface_alt" => th.surface_alt,
        "border" => th.border,
        "accent" => th.accent,
        "accent_warm" => th.accent_warm,
        "fg" => th.fg,
        "fg_muted" => th.fg_muted,
        "fg_dim" => th.fg_dim,
        "success" => th.success,
        "warning" => th.warning,
        "error" => th.error,
        "selection" => th.selection,
        "cursor_line" => th.cursor_line,
        "range" => th.range,
        "mode_normal" => th.mode_normal,
        "mode_insert" => th.mode_insert,
        "mode_visual" => th.mode_visual,
        "mode_command" => th.mode_command,
        "mode_fg" => th.mode_fg,
        _ => return None,
    };
    color_text(c)
}

/// Style token `token` of `th` as a theme file writes it, and whether `th` sets it (else it is
/// the look the theme works out now); `None`: one it cannot hold.
pub(super) fn style_value(th: &Theme, token: &str) -> Option<(String, bool)> {
    let mut styles = th.styles;
    let set = (*styles.slot(token)?).is_some();
    let s = match token {
        "selection" => th.styles.selection.unwrap_or_else(|| Style::new().bg(th.selection)),
        "cursor_line" => th.styles.cursor_line.unwrap_or_else(|| Style::new().bg(th.cursor_line)),
        "link" => th.link(),
        "mention" => th.mention(false),
        "badge" => th.badge(),
        "current" => th.current(),
        "code" => th.code(),
        "reaction" => th.reaction(),
        "reaction_mine" => th.reaction_mine(),
        "tab_active" => th.tab(true),
        "tab_badge" => th.tab_badge(false, false),
        "status_line" => th.status_line(),
        "hint" => th.hint(),
        _ => th.search_match(),
    };
    Some((style_text(s)?, set))
}

/// A colour as a theme file writes it (quoted); `None`: one it cannot hold.
fn color_text(c: Color) -> Option<String> {
    let text = match c {
        Color::Reset => "default".to_string(),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        c => ANSI_NAMES[ANSI.iter().position(|a| *a == c)?].to_string(),
    };
    Some(format!("\"{text}\""))
}

/// The modifier of a theme file's modifier.
fn modifier(m: ModifierSpec) -> Modifier {
    match m {
        ModifierSpec::Bold => Modifier::BOLD,
        ModifierSpec::Dim => Modifier::DIM,
        ModifierSpec::Italic => Modifier::ITALIC,
        ModifierSpec::Underlined => Modifier::UNDERLINED,
        ModifierSpec::Reversed => Modifier::REVERSED,
        ModifierSpec::CrossedOut => Modifier::CROSSED_OUT,
    }
}

/// A style as a theme file writes it (an inline table); `None`: one it cannot hold (a modifier
/// with no name in the file format). What a style takes away or underlines in colour is left
/// out: a theme file cannot say it.
fn style_text(s: Style) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(fg) = s.fg {
        parts.push(format!("fg = {}", color_text(fg)?));
    }
    if let Some(bg) = s.bg {
        parts.push(format!("bg = {}", color_text(bg)?));
    }
    let all = ModifierSpec::ALL.iter().fold(Modifier::empty(), |acc, (_, m)| acc | modifier(*m));
    if !all.contains(s.add_modifier) {
        return None;
    }
    let named: Vec<String> = ModifierSpec::ALL
        .iter()
        .filter(|(_, m)| s.add_modifier.contains(modifier(*m)))
        .map(|(n, _)| format!("\"{n}\""))
        .collect();
    if !named.is_empty() {
        parts.push(format!("modifiers = [{}]", named.join(", ")));
    }
    Some(if parts.is_empty() { "{}".to_string() } else { format!("{{ {} }}", parts.join(", ")) })
}

#[cfg(test)]
mod tests;
