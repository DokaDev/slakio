//! A theme file made a theme: the built-in theme it `extends` (`terminal` without one; a family
//! by the terminal's background) with the tokens the file sets.

use super::{Dim, Kind, Look, NAMES, Theme, resolve};
use ratatui::style::{Color, Modifier, Style};
use slakio_core::theme_file::{self, ColorSpec, DimSpec, ModifierSpec, StyleSpec, ThemeError, ThemeSpec};
use std::borrow::Cow;
use std::path::Path;

/// The theme the setting `name` draws with here: a built-in one (or `auto`), else the theme file
/// `<dir>/<name>.toml`. `NO_COLOR` still draws without colour, the file read and checked all the
/// same. Never creates anything.
pub fn load(name: &str, look: Look, dir: Option<&Path>) -> Result<Theme, ThemeError> {
    let lower = name.to_ascii_lowercase();
    if NAMES.contains(&lower.as_str()) {
        return Ok(look.theme(&lower));
    }
    if !theme_file::valid_name(name) {
        return Err(ThemeError::BadName(name.to_string()));
    }
    let Some(dir) = dir else { return Err(ThemeError::Unknown { name: name.to_string(), dir: None }) };
    let spec = theme_file::read(dir, name)?;
    if look.no_color {
        return Ok(Theme::no_color());
    }
    Ok(from_spec(name, &spec, look))
}

/// The theme `name` of the file read as `spec`, on a terminal `look`.
pub fn from_spec(name: &str, spec: &ThemeSpec, look: Look) -> Theme {
    let base = spec.extends.as_ref().map_or("terminal", |(b, _)| b.as_str());
    let mut t = resolve(base, look.truecolor, look.background);
    t.name = Cow::Owned(name.to_string());
    // 24-bit where the base theme is, or where the file names an `#rrggbb` colour.
    if spec.uses_rgb() {
        t.kind = Kind::Truecolor;
    }
    for (token, c) in &spec.colors {
        if let Some(slot) = color_slot(&mut t, token) {
            *slot = color(*c);
        }
    }
    t.derive_reactions(|token| spec.colors.iter().any(|(t, _)| *t == token));
    if let Some(list) = &spec.workspaces {
        t.workspaces = Cow::Owned(list.iter().copied().map(color).collect());
    }
    if let Some(list) = &spec.avatars {
        t.avatars = Cow::Owned(list.iter().copied().map(color).collect());
    }
    if let Some(tint) = spec.avatar_tint {
        t.avatar_tint = tint;
    }
    if let Some(dim) = spec.dim {
        t.dim = match dim {
            DimSpec::Modifier => Dim::Modifier,
            DimSpec::Blend { toward: (r, g, b), keep } => Dim::Blend { toward: Color::Rgb(r, g, b), keep },
        };
    }
    for (token, s) in &spec.styles {
        if let Some(slot) = t.styles.slot(token) {
            *slot = Some(style(s));
        }
    }
    t
}

/// The field of colour token `name` (`None`: not one of [`theme_file::COLOR_TOKENS`]).
fn color_slot<'a>(t: &'a mut Theme, name: &str) -> Option<&'a mut Color> {
    Some(match name {
        "bg" => &mut t.bg,
        "surface" => &mut t.surface,
        "surface_alt" => &mut t.surface_alt,
        "border" => &mut t.border,
        "accent" => &mut t.accent,
        "accent_warm" => &mut t.accent_warm,
        "fg" => &mut t.fg,
        "fg_muted" => &mut t.fg_muted,
        "fg_dim" => &mut t.fg_dim,
        "success" => &mut t.success,
        "warning" => &mut t.warning,
        "error" => &mut t.error,
        "selection" => &mut t.selection,
        "cursor_line" => &mut t.cursor_line,
        "range" => &mut t.range,
        "reaction" => &mut t.reaction,
        "reaction_mine" => &mut t.reaction_mine,
        "mode_normal" => &mut t.mode_normal,
        "mode_insert" => &mut t.mode_insert,
        "mode_visual" => &mut t.mode_visual,
        "mode_command" => &mut t.mode_command,
        "mode_fg" => &mut t.mode_fg,
        _ => return None,
    })
}

/// The colours of the 16 ANSI names, in [`theme_file::ANSI_NAMES`] order.
pub(super) const ANSI: [Color; 16] = [
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::Gray,
    Color::DarkGray,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
];

/// The colour of a theme file's colour.
pub fn color(c: ColorSpec) -> Color {
    match c {
        ColorSpec::Default => Color::Reset,
        ColorSpec::Ansi(i) => ANSI[usize::from(i) % ANSI.len()],
        ColorSpec::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// The style of a theme file's style: what it leaves out is not set.
fn style(s: &StyleSpec) -> Style {
    let mut out = Style::new();
    if let Some(fg) = s.fg {
        out = out.fg(color(fg));
    }
    if let Some(bg) = s.bg {
        out = out.bg(color(bg));
    }
    for m in &s.modifiers {
        out = out.add_modifier(match m {
            ModifierSpec::Bold => Modifier::BOLD,
            ModifierSpec::Dim => Modifier::DIM,
            ModifierSpec::Italic => Modifier::ITALIC,
            ModifierSpec::Underlined => Modifier::UNDERLINED,
            ModifierSpec::Reversed => Modifier::REVERSED,
            ModifierSpec::CrossedOut => Modifier::CROSSED_OUT,
        });
    }
    out
}

#[cfg(test)]
mod tests;
