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

/// Light: dark text on an off-white background.
pub const LIGHT: Theme = Theme {
    name: "light",
    kind: Kind::Truecolor,
    bg: rgb(0xFBFBFC),
    surface: rgb(0xF1F3F6),
    surface_alt: rgb(0xF4F5F7),
    border: rgb(0xC9CED6),
    accent: rgb(0x0A7E78),
    accent_warm: rgb(0xA45A00),
    fg: rgb(0x1F2328),
    fg_muted: rgb(0x59636E),
    fg_dim: rgb(0x818B98),
    success: rgb(0x1A7F37),
    warning: rgb(0x8A6100),
    error: rgb(0xC0262D),
    selection: rgb(0xD5E3F0),
    cursor_line: rgb(0xEBEEF3),
    range: rgb(0xDDEDEA),
    mode_normal: rgb(0x0550AE),
    mode_insert: rgb(0x1A7F37),
    mode_visual: rgb(0x8250DF),
    mode_command: rgb(0xBC4C00),
    mode_fg: rgb(0xFFFFFF),
    workspaces: &WORKSPACE_COLORS,
    dim: Dim::Blend { toward: rgb(0x808080), keep: 55 },
};

/// White text on black, saturated marks, a strong selection.
pub const HIGH_CONTRAST: Theme = Theme {
    name: "high-contrast",
    kind: Kind::Truecolor,
    bg: rgb(0x0A0A0A),
    surface: rgb(0x1C1C1C),
    surface_alt: rgb(0x141414),
    border: rgb(0xFFFFFF),
    accent: rgb(0x00E5FF),
    accent_warm: rgb(0xFFB000),
    fg: rgb(0xFFFFFF),
    fg_muted: rgb(0xD0D0D0),
    fg_dim: rgb(0xA8A8A8),
    success: rgb(0x00FF66),
    warning: rgb(0xFFFF00),
    error: rgb(0xFF4040),
    selection: rgb(0x0033CC),
    cursor_line: rgb(0x262626),
    range: rgb(0x004D40),
    mode_normal: rgb(0x66B3FF),
    mode_insert: rgb(0x00FF66),
    mode_visual: rgb(0xFF66FF),
    mode_command: rgb(0xFFB000),
    mode_fg: rgb(0x000000),
    workspaces: &WORKSPACE_COLORS,
    dim: Dim::Blend { toward: rgb(0x000000), keep: 55 },
};

// Catppuccin (github.com/catppuccin/catppuccin, MIT): the palettes of Mocha and Latte.
/// Catppuccin Mocha, the dark flavor.
pub const CATPPUCCIN_MOCHA: Theme = Theme {
    name: "catppuccin-mocha",
    kind: Kind::Truecolor,
    bg: rgb(0x1E1E2E),
    surface: rgb(0x313244),
    surface_alt: rgb(0x181825),
    border: rgb(0x45475A),
    accent: rgb(0x89B4FA),
    accent_warm: rgb(0xFAB387),
    fg: rgb(0xCDD6F4),
    fg_muted: rgb(0x7F849C),
    fg_dim: rgb(0x6C7086),
    success: rgb(0xA6E3A1),
    warning: rgb(0xF9E2AF),
    error: rgb(0xF38BA8),
    selection: rgb(0x45475A),
    cursor_line: rgb(0x2A2B3C),
    range: rgb(0x2B3A52),
    mode_normal: rgb(0x89B4FA),
    mode_insert: rgb(0xA6E3A1),
    mode_visual: rgb(0xCBA6F7),
    mode_command: rgb(0xFAB387),
    mode_fg: rgb(0x11111B),
    workspaces: &WORKSPACE_COLORS,
    dim: Dim::Blend { toward: rgb(0x101019), keep: 57 },
};

/// Catppuccin Latte, the light flavor. Moved from the palette so text and marks keep their
/// contrast: `warning` (palette `#DF8E1D`), `success` (palette `#40A02B`), `accent_warm`
/// (palette `#FE640B`), `error` (palette `#D20F39`, lightened for the count pills).
pub const CATPPUCCIN_LATTE: Theme = Theme {
    name: "catppuccin-latte",
    kind: Kind::Truecolor,
    bg: rgb(0xEFF1F5),
    surface: rgb(0xE6E9EF),
    surface_alt: rgb(0xDCE0E8),
    border: rgb(0xBCC0CC),
    accent: rgb(0x1E66F5),
    accent_warm: rgb(0xD85509),
    fg: rgb(0x4C4F69),
    fg_muted: rgb(0x6C6F85),
    fg_dim: rgb(0x8C8FA1),
    success: rgb(0x3A9027),
    warning: rgb(0xB07017),
    error: rgb(0xDC4465),
    selection: rgb(0xCCD0DA),
    cursor_line: rgb(0xE3E6ED),
    range: rgb(0xD4DCEE),
    mode_normal: rgb(0x209FB5),
    mode_insert: rgb(0x40A02B),
    mode_visual: rgb(0xEA76CB),
    mode_command: rgb(0xFE640B),
    mode_fg: rgb(0x11111B),
    workspaces: &WORKSPACE_COLORS,
    dim: Dim::Blend { toward: rgb(0x808080), keep: 78 },
};

// Gruvbox (github.com/morhetz/gruvbox, MIT).
/// Gruvbox dark (medium contrast). `error` is lightened from the palette's `#FB4934` for the
/// count pills.
pub const GRUVBOX_DARK: Theme = Theme {
    name: "gruvbox-dark",
    kind: Kind::Truecolor,
    bg: rgb(0x282828),
    surface: rgb(0x32302F),
    surface_alt: rgb(0x1D2021),
    border: rgb(0x504945),
    accent: rgb(0x83A598),
    accent_warm: rgb(0xFE8019),
    fg: rgb(0xEBDBB2),
    fg_muted: rgb(0xA89984),
    fg_dim: rgb(0x7C6F64),
    success: rgb(0xB8BB26),
    warning: rgb(0xFABD2F),
    error: rgb(0xFB5440),
    selection: rgb(0x504945),
    cursor_line: rgb(0x3C3836),
    range: rgb(0x45403D),
    mode_normal: rgb(0x83A598),
    mode_insert: rgb(0xB8BB26),
    mode_visual: rgb(0xD3869B),
    mode_command: rgb(0xFE8019),
    mode_fg: rgb(0x282828),
    workspaces: &WORKSPACE_COLORS,
    dim: Dim::Blend { toward: rgb(0x161616), keep: 57 },
};

/// Gruvbox light (medium contrast). Moved from the palette so text and marks keep their
/// contrast: `warning` (palette `#B57614`), `mode_insert` (palette `#79740E`).
pub const GRUVBOX_LIGHT: Theme = Theme {
    name: "gruvbox-light",
    kind: Kind::Truecolor,
    bg: rgb(0xFBF1C7),
    surface: rgb(0xF2E5BC),
    surface_alt: rgb(0xEBDBB2),
    border: rgb(0xD5C4A1),
    accent: rgb(0x076678),
    accent_warm: rgb(0xAF3A03),
    fg: rgb(0x3C3836),
    fg_muted: rgb(0x7C6F64),
    fg_dim: rgb(0x928374),
    success: rgb(0x79740E),
    warning: rgb(0xAA7016),
    error: rgb(0x9D0006),
    selection: rgb(0xD5C4A1),
    cursor_line: rgb(0xEBDBB2),
    range: rgb(0xE4D6AE),
    mode_normal: rgb(0x076678),
    mode_insert: rgb(0x716C11),
    mode_visual: rgb(0x8F3F71),
    mode_command: rgb(0xAF3A03),
    mode_fg: rgb(0xFBF1C7),
    workspaces: &WORKSPACE_COLORS,
    dim: Dim::Blend { toward: rgb(0x606060), keep: 60 },
};

// Nord (nordtheme.com, MIT).
/// Nord. Moved from the palette so text and marks keep their contrast: `error` (palette
/// `#BF616A`), `mode_fg` (palette `#2E3440`).
pub const NORD: Theme = Theme {
    name: "nord",
    kind: Kind::Truecolor,
    bg: rgb(0x2E3440),
    surface: rgb(0x3B4252),
    surface_alt: rgb(0x353B49),
    border: rgb(0x4C566A),
    accent: rgb(0x88C0D0),
    accent_warm: rgb(0xD08770),
    fg: rgb(0xD8DEE9),
    fg_muted: rgb(0x9AA3B5),
    fg_dim: rgb(0x616E88),
    success: rgb(0xA3BE8C),
    warning: rgb(0xEBCB8B),
    error: rgb(0xC7767D),
    selection: rgb(0x434C5E),
    cursor_line: rgb(0x3B4252),
    range: rgb(0x3B4A5A),
    mode_normal: rgb(0x88C0D0),
    mode_insert: rgb(0xA3BE8C),
    mode_visual: rgb(0xB48EAD),
    mode_command: rgb(0xD08770),
    mode_fg: rgb(0x22262F),
    workspaces: &WORKSPACE_COLORS,
    dim: Dim::Blend { toward: rgb(0x191D23), keep: 61 },
};

// Dracula (draculatheme.com, github.com/dracula/dracula-theme, MIT).
/// Dracula.
pub const DRACULA: Theme = Theme {
    name: "dracula",
    kind: Kind::Truecolor,
    bg: rgb(0x282A36),
    surface: rgb(0x343746),
    surface_alt: rgb(0x21222C),
    border: rgb(0x44475A),
    accent: rgb(0xBD93F9),
    accent_warm: rgb(0xFFB86C),
    fg: rgb(0xF8F8F2),
    fg_muted: rgb(0x8C96BF),
    fg_dim: rgb(0x6272A4),
    success: rgb(0x50FA7B),
    warning: rgb(0xF1FA8C),
    error: rgb(0xFF5555),
    selection: rgb(0x44475A),
    cursor_line: rgb(0x343746),
    range: rgb(0x3B3F55),
    mode_normal: rgb(0xBD93F9),
    mode_insert: rgb(0x50FA7B),
    mode_visual: rgb(0xFF79C6),
    mode_command: rgb(0xFFB86C),
    mode_fg: rgb(0x282A36),
    workspaces: &WORKSPACE_COLORS,
    dim: Dim::Blend { toward: rgb(0x16171E), keep: 55 },
};

/// The built-in themes by name.
pub const BUILTINS: &[&Theme] = &[
    &TERMINAL,
    &DARK,
    &LIGHT,
    &HIGH_CONTRAST,
    &CATPPUCCIN_LATTE,
    &CATPPUCCIN_MOCHA,
    &TOKYO_NIGHT_DAY,
    &TOKYO_NIGHT_NIGHT,
    &GRUVBOX_LIGHT,
    &GRUVBOX_DARK,
    &NORD,
    &DRACULA,
];

/// A name that picks the light or the dark variant by the terminal's background (the dark one
/// when that is not known).
pub const FAMILIES: &[(&str, &Theme, &Theme)] = &[
    ("catppuccin", &CATPPUCCIN_LATTE, &CATPPUCCIN_MOCHA),
    ("tokyo-night", &TOKYO_NIGHT_DAY, &TOKYO_NIGHT_NIGHT),
    ("gruvbox", &GRUVBOX_LIGHT, &GRUVBOX_DARK),
];

/// Every value the `theme` setting takes, in the order lists show them.
pub const NAMES: &[&str] = &[
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
