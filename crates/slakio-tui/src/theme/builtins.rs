//! The built-in themes and how the `theme` setting picks one.

use super::{Background, Dim, Kind, Theme, rgb};
use ratatui::style::Color;

/// Workspace colors of the truecolor themes: twelve that read on a dark background and stay
/// apart from each other.
pub const WORKSPACE_COLORS: [Color; 12] = [
    rgb(0xE06C75),
    rgb(0xE5935A),
    rgb(0xE6C35C),
    rgb(0x8FC77A),
    rgb(0x4FB3A9),
    rgb(0x5CC6D9),
    rgb(0x5B8DEF),
    rgb(0x8C8CF0),
    rgb(0xC792EA),
    rgb(0xF08CC0),
    rgb(0xB8906A),
    rgb(0x9AA3B5),
];

/// Workspace colors of the ANSI theme.
pub const ANSI_WORKSPACE_COLORS: [Color; 6] =
    [Color::Green, Color::Magenta, Color::Cyan, Color::Yellow, Color::Blue, Color::Red];

/// The terminal's own 16 colors, so it follows the user's palette. Muted text is ANSI 7 on a
/// dark background; [`resolve`] makes it ANSI 8 on a light one.
pub const TERMINAL: Theme = Theme {
    name: "terminal",
    kind: Kind::Ansi,
    bg: Color::Reset,
    surface: Color::Reset,
    surface_alt: Color::Reset,
    border: Color::DarkGray,
    accent: Color::Cyan,
    accent_warm: Color::Magenta,
    fg: Color::Reset,
    fg_muted: Color::Gray,
    fg_dim: Color::DarkGray,
    success: Color::Green,
    warning: Color::Yellow,
    error: Color::Red,
    selection: Color::DarkGray,
    cursor_line: Color::Reset,
    range: Color::DarkGray,
    mode_normal: Color::Blue,
    mode_insert: Color::Green,
    mode_visual: Color::Magenta,
    mode_command: Color::Yellow,
    mode_fg: Color::Black,
    workspaces: &ANSI_WORKSPACE_COLORS,
    dim: Dim::Modifier,
};

/// The dark truecolor theme.
pub const DARK: Theme = Theme {
    name: "dark",
    kind: Kind::Truecolor,
    bg: rgb(0x14161B),
    surface: rgb(0x1B1E25),
    surface_alt: rgb(0x20242C),
    border: rgb(0x2E3440),
    accent: rgb(0x4FB3A9),
    accent_warm: rgb(0xE0A96D),
    fg: rgb(0xD8DEE9),
    fg_muted: rgb(0x8A93A6),
    fg_dim: rgb(0x5C6577),
    success: rgb(0x8FC77A),
    warning: rgb(0xE6C35C),
    error: rgb(0xE06C75),
    selection: rgb(0x2A3B4D),
    cursor_line: rgb(0x1F2530),
    range: rgb(0x1E3438),
    mode_normal: rgb(0x61AFEF),
    mode_insert: rgb(0x98C379),
    mode_visual: rgb(0xC678DD),
    mode_command: rgb(0xE5935A),
    mode_fg: rgb(0x14161B),
    workspaces: &WORKSPACE_COLORS,
    dim: Dim::Blend { toward: rgb(0x0B0C10), keep: 55 },
};

// Tokyo Night (github.com/folke/tokyonight.nvim, Apache-2.0).
/// Tokyo Night, the night style.
pub const TOKYO_NIGHT_NIGHT: Theme = Theme {
    name: "tokyo-night-night",
    kind: Kind::Truecolor,
    bg: rgb(0x1A1B26),
    surface: rgb(0x1F2335),
    surface_alt: rgb(0x16161E),
    border: rgb(0x414868),
    accent: rgb(0x7AA2F7),
    accent_warm: rgb(0xFF9E64),
    fg: rgb(0xC0CAF5),
    fg_muted: rgb(0x737AA2),
    fg_dim: rgb(0x565F89),
    success: rgb(0x9ECE6A),
    warning: rgb(0xE0AF68),
    error: rgb(0xF7768E),
    selection: rgb(0x394B70),
    cursor_line: rgb(0x292E42),
    range: rgb(0x283457),
    mode_normal: rgb(0x7AA2F7),
    mode_insert: rgb(0x9ECE6A),
    mode_visual: rgb(0xBB9AF7),
    mode_command: rgb(0xFF9E64),
    mode_fg: rgb(0x15161E),
    workspaces: &WORKSPACE_COLORS,
    dim: Dim::Blend { toward: rgb(0x0E0F15), keep: 60 },
};

/// Tokyo Night, the day style. Text and marks are moved from the palette so they keep their
/// contrast on the light background.
pub const TOKYO_NIGHT_DAY: Theme = Theme {
    name: "tokyo-night-day",
    kind: Kind::Truecolor,
    bg: rgb(0xE1E2E7),
    surface: rgb(0xECEEF3),
    surface_alt: rgb(0xD8DAE3),
    border: rgb(0xA8AECB),
    accent: rgb(0x2D7AE3),
    accent_warm: rgb(0xB15C00),
    fg: rgb(0x2E4B91),
    fg_muted: rgb(0x68709A),
    fg_dim: rgb(0x848CB5),
    success: rgb(0x587539),
    warning: rgb(0x8C6C3E),
    error: rgb(0xB42F2F),
    selection: rgb(0xB7C1E3),
    cursor_line: rgb(0xD0D5E3),
    range: rgb(0xC4C8DA),
    mode_normal: rgb(0x2759A1),
    mode_insert: rgb(0x4A6135),
    mode_visual: rgb(0xB1055A),
    mode_command: rgb(0x884A0A),
    mode_fg: rgb(0xE1E2E7),
    workspaces: &WORKSPACE_COLORS,
    dim: Dim::Blend { toward: rgb(0x808080), keep: 79 },
};

/// The built-in themes by name.
pub const BUILTINS: &[&Theme] = &[&TERMINAL, &DARK, &TOKYO_NIGHT_NIGHT, &TOKYO_NIGHT_DAY];

/// A name that picks the light or the dark variant by the terminal's background (the dark one
/// when that is not known).
pub const FAMILIES: &[(&str, &Theme, &Theme)] = &[("tokyo-night", &TOKYO_NIGHT_DAY, &TOKYO_NIGHT_NIGHT)];

/// Every value the `theme` setting takes.
pub const NAMES: &[&str] = &["auto", "terminal", "dark", "tokyo-night", "tokyo-night-night", "tokyo-night-day"];

/// The truecolor theme `auto` picks.
pub const AUTO_TRUECOLOR: &str = "tokyo-night";

/// The theme of setting `name` on a terminal that shows 24-bit color (`truecolor`) or not, with
/// a `background`. `auto` is [`AUTO_TRUECOLOR`] with 24-bit color, else `terminal`.
pub fn resolve(name: &str, truecolor: bool, background: Background) -> Theme {
    let name = match name {
        "auto" if truecolor => AUTO_TRUECOLOR,
        "auto" => "terminal",
        n => n,
    };
    let picked = FAMILIES
        .iter()
        .find(|(n, ..)| *n == name)
        .map(|(_, light, dark)| if background == Background::Light { *light } else { *dark })
        .or_else(|| BUILTINS.iter().copied().find(|t| t.name == name))
        .unwrap_or(&TERMINAL);
    let mut theme = picked.clone();
    if theme.kind == Kind::Ansi && background == Background::Light {
        theme.fg_muted = Color::DarkGray;
    }
    theme
}
