//! The theme: the color tokens every widget draws with, and the styles made of them. Widgets
//! never name a color of their own; they ask the theme for a role ([`Theme::text`],
//! [`Theme::title`], [`Theme::badge`], …).
//!
//! The built-ins ([`BUILTINS`]): `terminal` (the terminal's own 16 ANSI
//! colors, so it follows the user's palette), `dark`, `light`, `high-contrast`, `nord`,
//! `dracula`, and three families that take their light or dark variant by the terminal's
//! background: `catppuccin` (`-latte` / `-mocha`), `tokyo-night` (`-day` / `-night`) and
//! `gruvbox` (`-light` / `-dark`). The setting `theme = "auto"` (the default)
//! takes `tokyo-night` on a terminal that says it shows 24-bit color (`COLORTERM` is `truecolor`
//! or `24bit`), else `terminal` ([`resolve`]). With `NO_COLOR` set (to anything but the empty
//! string, <https://no-color.org>) [`Theme::no_color`] draws without any color: everything a
//! color marks also has a shape (a text badge, a letter, a dot, a bar, reversed or bold text),
//! so nothing is told by color alone.
//!
//! Rules every widget follows:
//! * the focus shows on a panel's border and title only; nothing inside a panel changes color
//!   with the focus, except the selection bar;
//! * a selection is a background (or, without one, a bar in the left gutter), never an
//!   underline.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use slakio_core::model::{Presence, WorkspaceColor};

mod builtins;
pub use builtins::*;

/// `c` with `keep` percent of its brightness (toward black).
fn darken(c: Color, keep: u16) -> Color {
    let Color::Rgb(r, g, b) = c else { return c };
    let k = |v: u8| (u16::from(v) * keep / 100) as u8;
    Color::Rgb(k(r), k(g), k(b))
}

const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// How a theme can show things: with 24-bit colors, with the 16 ANSI colors, or with none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Truecolor,
    Ansi,
    NoColor,
}

/// How the screen behind a dialog is dimmed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dim {
    /// Blend every RGB color toward `toward`, keeping `keep` percent of it.
    Blend { toward: Color, keep: u16 },
    /// Add [`Modifier::DIM`] and keep the colors (for themes of terminal colors).
    Modifier,
}

/// The terminal's background, asked once at startup (OSC 11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Background {
    Light,
    Dark,
    /// No answer, or not asked.
    #[default]
    Unknown,
}

/// What decides how a theme name is drawn on this terminal: 24-bit color or not, its
/// background, and `NO_COLOR`. Read once at startup, kept for a theme picked later.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    pub truecolor: bool,
    pub background: Background,
    pub no_color: bool,
}

impl Look {
    /// From the environment `env` (`COLORTERM`, `NO_COLOR`) and the terminal's `background`.
    pub fn from_env(env: impl Fn(&str) -> Option<String>, background: Background) -> Self {
        Self {
            truecolor: env("COLORTERM")
                .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "truecolor" | "24bit")),
            background,
            no_color: env("NO_COLOR").is_some_and(|v| !v.is_empty()),
        }
    }

    /// The theme the setting `name` draws with here: none at all under `NO_COLOR`.
    pub fn theme(self, name: &str) -> Theme {
        if self.no_color { Theme::no_color() } else { resolve(name, self.truecolor, self.background) }
    }
}

/// The tokens of a theme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,
    pub kind: Kind,
    /// The app's background.
    pub bg: Color,
    /// The status line, popups.
    pub surface: Color,
    /// Reaction pills.
    pub surface_alt: Color,
    /// Borders of panels without the focus, dividers.
    pub border: Color,
    /// The focused panel's border, links, prompts, the current rail item, keys in popups.
    pub accent: Color,
    /// Key groups and keys of an empty state, the backend's state.
    pub accent_warm: Color,
    /// Body text.
    pub fg: Color,
    /// Section headers, titles without the focus, hint labels.
    pub fg_muted: Color,
    /// Times, date lines, muted conversations, placeholders.
    pub fg_dim: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    /// The background of the selected row of the focused panel.
    pub selection: Color,
    /// The background of the selected row of a panel without the focus (a truecolor theme;
    /// the others draw a bar in the gutter).
    pub cursor_line: Color,
    /// The background of a VISUAL range.
    pub range: Color,
    /// Mode badges (with [`Self::mode_fg`] bold on them), each also naming its mode.
    pub mode_normal: Color,
    pub mode_insert: Color,
    pub mode_visual: Color,
    pub mode_command: Color,
    pub mode_fg: Color,
    /// Workspace colors, by [`WorkspaceColor`] slot (wrapping around).
    pub workspaces: &'static [Color],
    /// Avatar chips, a person's by a slot of their id ([`crate::avatar::slot`]), under
    /// [`Self::mode_fg`] text.
    pub avatars: &'static [Color],
    /// Percent of an avatar hue mixed into the background for its chip, the hue drawing the
    /// initials (a dark theme: a quiet chip); `0` draws the hue solid under [`Self::mode_fg`].
    pub avatar_tint: u16,
    pub dim: Dim,
}

/// The gutter mark of a selection drawn without a background.
pub const GUTTER: &str = "▎";

/// How a selected row is painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    /// The selected row of the panel with the focus.
    Focused,
    /// The selected row of a panel without the focus.
    Unfocused,
    /// A row of a VISUAL range.
    Visual,
}

impl Theme {
    /// No color at all (`NO_COLOR`): shapes only.
    pub fn no_color() -> Self {
        Self {
            name: "no-color",
            kind: Kind::NoColor,
            workspaces: &[Color::Reset],
            avatars: &[Color::Reset],
            avatar_tint: 0,
            dim: Dim::Modifier,
            ..TERMINAL
        }
        .without_colors()
    }

    fn without_colors(mut self) -> Self {
        for c in [
            &mut self.bg,
            &mut self.surface,
            &mut self.surface_alt,
            &mut self.border,
            &mut self.accent,
            &mut self.accent_warm,
            &mut self.fg,
            &mut self.fg_muted,
            &mut self.fg_dim,
            &mut self.success,
            &mut self.warning,
            &mut self.error,
            &mut self.selection,
            &mut self.cursor_line,
            &mut self.range,
            &mut self.mode_normal,
            &mut self.mode_insert,
            &mut self.mode_visual,
            &mut self.mode_command,
            &mut self.mode_fg,
        ] {
            *c = Color::Reset;
        }
        self
    }

    /// The default theme: the terminal's own colors (no environment asked).
    pub fn terminal() -> Self {
        TERMINAL
    }

    /// The theme for the setting `name` (`auto` or a built-in), the environment `env` and the
    /// terminal's `background`. `NO_COLOR` wins over everything; an unknown name is `auto`
    /// (the config file check rejects it before).
    pub fn from_env(name: &str, env: impl Fn(&str) -> Option<String>, background: Background) -> Self {
        Look::from_env(env, background).theme(name)
    }

    fn plain(&self) -> bool {
        self.kind == Kind::NoColor
    }

    /// The app's background and text, under everything drawn.
    pub fn base(&self) -> Style {
        Style::new().fg(self.fg).bg(self.bg)
    }

    pub fn text(&self) -> Style {
        Style::new().fg(self.fg)
    }

    pub fn muted(&self) -> Style {
        Style::new().fg(self.fg_muted)
    }

    /// Times, date lines, placeholders, muted conversations.
    pub fn faint(&self) -> Style {
        if self.plain() { Style::new().add_modifier(Modifier::DIM) } else { Style::new().fg(self.fg_dim) }
    }

    pub fn bold(&self) -> Style {
        self.text().add_modifier(Modifier::BOLD)
    }

    /// A panel's title: bold body text with the focus, muted without.
    pub fn title(&self, focused: bool) -> Style {
        if focused { self.bold() } else { self.muted() }
    }

    /// A panel's border.
    pub fn border(&self, focused: bool) -> Style {
        match (self.plain(), focused) {
            (true, true) => Style::new().add_modifier(Modifier::BOLD),
            (true, false) => Style::new(),
            (false, true) => Style::new().fg(self.accent),
            (false, false) => Style::new().fg(self.border),
        }
    }

    /// Dividers and the status line's separators.
    pub fn divider(&self) -> Style {
        Style::new().fg(self.border)
    }

    pub fn section(&self) -> Style {
        self.muted().add_modifier(Modifier::BOLD)
    }

    /// The count pill of mentions and unread DMs.
    pub fn badge(&self) -> Style {
        if self.plain() {
            return Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD);
        }
        Style::new().fg(self.mode_fg).bg(self.error).add_modifier(Modifier::BOLD)
    }

    /// An unread mark that is not a count (`●`): plain, or a mention's color.
    pub fn dot(&self, mention: bool) -> Style {
        if mention && !self.plain() { Style::new().fg(self.error).add_modifier(Modifier::BOLD) } else { self.bold() }
    }

    /// A person's presence mark: active in the success color, away muted, do not disturb in the
    /// warning color. The marks differ in shape too ([`crate::ui`]), so no color is needed.
    pub fn presence(&self, p: Presence) -> Style {
        match p {
            Presence::Active => Style::new().fg(self.success),
            Presence::Dnd => Style::new().fg(self.warning),
            Presence::Away | Presence::Unknown => self.muted(),
        }
    }

    /// A presence mark on a selected row, painted over the bar after it ([`Self::paint_selection`]
    /// lifts muted text). The 16-color theme keeps the mark's own color, unless it is the bar's
    /// (a light terminal's muted gray): every color there reads on the dark gray bar. A truecolor
    /// bar lifts the away mark like other muted text; the other marks keep their color anyway.
    pub fn presence_selected(&self, p: Presence) -> Option<Color> {
        let fg = self.presence(p).fg?;
        (self.kind == Kind::Ansi && fg != self.selection).then_some(fg)
    }

    pub fn accent(&self) -> Style {
        Style::new().fg(self.accent)
    }

    /// The current workspace or view on the rail.
    pub fn current(&self) -> Style {
        if self.plain() { self.bold() } else { self.accent().add_modifier(Modifier::BOLD) }
    }

    /// The backend's state (`demo`) and other warm marks.
    pub fn warm(&self) -> Style {
        Style::new().fg(self.accent_warm)
    }

    /// A key in an empty state's key list.
    pub fn key_warm(&self) -> Style {
        self.warm().add_modifier(Modifier::BOLD)
    }

    /// A key in a popup or the hint line.
    pub fn key(&self) -> Style {
        if self.plain() { self.bold() } else { self.accent().add_modifier(Modifier::BOLD) }
    }

    pub fn author(&self) -> Style {
        self.bold()
    }

    pub fn own_author(&self) -> Style {
        if self.plain() {
            return Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
        }
        self.accent().add_modifier(Modifier::BOLD)
    }

    /// The tab bar's row, under the tabs.
    pub fn tab_bar(&self) -> Style {
        self.base()
    }

    /// A tab's title (and the spaces of its label): the tab shown on the raised surface, in
    /// bold; the others muted.
    pub fn tab(&self, shown: bool) -> Style {
        match (self.plain(), shown) {
            (true, true) => Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            (true, false) => Style::new(),
            (false, true) => self.bold().bg(self.surface_alt),
            (false, false) => self.muted().bg(self.bg),
        }
    }

    /// A tab's number: the accent on the tab shown.
    pub fn tab_number(&self, shown: bool) -> Style {
        if shown && !self.plain() { self.tab(true).fg(self.accent) } else { self.tab(shown) }
    }

    /// A tab's close button: quiet, a little less on the tab shown.
    pub fn tab_close(&self, shown: bool) -> Style {
        match (self.plain(), shown) {
            (true, _) => self.tab(shown),
            (false, true) => self.tab(true).fg(self.fg_muted).remove_modifier(Modifier::BOLD),
            (false, false) => self.tab(false).fg(self.fg_dim),
        }
    }

    /// A tab's unread count (`●3`): a mention's color when one mentions the user.
    pub fn tab_badge(&self, shown: bool, mention: bool) -> Style {
        let dot = self.dot(mention);
        match self.tab(shown).bg {
            Some(bg) => dot.bg(bg),
            None => dot,
        }
    }

    /// The `‹` / `›` marks of tabs left out of the bar, by the strongest thing the tabs they
    /// hide hold: 2 a mention (its color), 1 unread (bold body text), 0 nothing (the accent).
    pub fn tab_more(&self, level: u8) -> Style {
        match level {
            0 => self.key(),
            l => self.dot(l > 1),
        }
    }

    /// The "N replies" row of a message with a thread.
    pub fn link(&self) -> Style {
        self.accent()
    }

    pub fn reaction(&self) -> Style {
        match self.kind {
            Kind::Truecolor => self.muted().bg(self.surface_alt),
            _ => self.muted(),
        }
    }

    pub fn reaction_mine(&self) -> Style {
        self.reaction().fg(self.accent).add_modifier(Modifier::BOLD)
    }

    pub fn warning(&self) -> Style {
        Style::new().fg(self.warning).add_modifier(Modifier::BOLD)
    }

    /// The status line and popups.
    pub fn surface(&self) -> Style {
        Style::new().fg(self.fg).bg(self.surface)
    }

    /// The mode badge of `bg` (one of the `mode_*` colors).
    pub fn mode(&self, bg: Color) -> Style {
        if self.plain() {
            return Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD);
        }
        Style::new().fg(self.mode_fg).bg(bg).add_modifier(Modifier::BOLD)
    }

    /// A person's avatar chip, of color slot `slot`: their initials bold in their color on a
    /// tint of it (darkened on a light background, for 4.5:1), solid under [`Self::mode_fg`]
    /// where a theme has no tint; colored initials alone in the 16-color theme (a solid block
    /// there is a pill); reversed without colors.
    pub fn avatar(&self, slot: usize) -> Style {
        let hue = self.avatars[slot % self.avatars.len()];
        let style = Style::new().add_modifier(Modifier::BOLD);
        match self.kind {
            Kind::NoColor => style.add_modifier(Modifier::REVERSED),
            Kind::Ansi => style.fg(hue),
            Kind::Truecolor => match self.tint(hue, self.avatar_tint) {
                Some(bg) if self.light() => style.fg(darken(hue, 80)).bg(bg),
                Some(bg) => style.fg(hue).bg(bg),
                None => style.fg(self.mode_fg).bg(hue),
            },
        }
    }

    /// The chip of a group DM (how many people are in it): body text on a tint of the muted
    /// color, visible on the background; muted bold text in the 16-color theme.
    pub fn avatar_group(&self) -> Style {
        match self.kind {
            Kind::NoColor => self.bold(),
            Kind::Ansi => self.muted().add_modifier(Modifier::BOLD),
            Kind::Truecolor => self.text().bg(self.neutral_chip()).add_modifier(Modifier::BOLD),
        }
    }

    /// The theme's background is light (an RGB one brighter than mid gray).
    fn light(&self) -> bool {
        matches!(self.bg, Color::Rgb(r, g, b) if u16::from(r) + u16::from(g) + u16::from(b) > 384)
    }

    /// The chip of a muted conversation: muted initials on the group chip's background.
    pub fn avatar_muted(&self) -> Style {
        match self.kind {
            Kind::NoColor => self.faint(),
            Kind::Ansi => self.muted(),
            Kind::Truecolor => self.muted().bg(self.neutral_chip()),
        }
    }

    /// The background of a neutral chip: the muted color mixed into the background.
    fn neutral_chip(&self) -> Color {
        self.tint(self.fg_muted, self.avatar_tint.max(25)).unwrap_or(self.surface_alt)
    }

    /// `percent` of `c` mixed into the background (`None`: no tint, or not RGB).
    fn tint(&self, c: Color, percent: u16) -> Option<Color> {
        let (Color::Rgb(r, g, b), Color::Rgb(br, bgc, bb)) = (c, self.bg) else { return None };
        if percent == 0 {
            return None;
        }
        let mix = |a: u8, z: u8| ((u16::from(a) * percent + u16::from(z) * (100 - percent)) / 100) as u8;
        Some(Color::Rgb(mix(r, br), mix(g, bgc), mix(b, bb)))
    }

    /// `c` is the background of an avatar chip (kept under a selection bar), and of nothing
    /// else a bar paints over: a surface that happens to be a chip's color is still a surface.
    fn chip_bg(&self, c: Color) -> bool {
        ![Color::Reset, self.bg, self.surface, self.surface_alt, self.selection].contains(&c)
            && ((0..self.avatars.len()).any(|i| self.avatar(i).bg == Some(c)) || self.avatar_group().bg == Some(c))
    }

    /// The style of workspace color `c`.
    pub fn workspace(&self, c: WorkspaceColor) -> Style {
        Style::new().fg(self.workspaces[usize::from(c.0) % self.workspaces.len()])
    }

    /// Paint `how` over the row `row` already drawn, whatever it is drawn on (the app's
    /// background, a popup's surface). Its first cell is the gutter: where the theme has no
    /// background for `how`, a bar there marks the row. Muted and faint text, and text the bar
    /// would hide, take the body color so it stays readable on the bar; bold, the pills (a
    /// background of the error color) and the avatar chips stay.
    pub fn paint_selection(&self, buf: &mut Buffer, row: Rect, how: Selection) {
        let row = row.intersection(buf.area);
        if row.is_empty() {
            return;
        }
        let bg = match (self.kind, how) {
            (Kind::NoColor, Selection::Unfocused) => None,
            (Kind::NoColor, _) => {
                for x in row.left()..row.right() {
                    buf[(x, row.y)].modifier.insert(Modifier::REVERSED);
                }
                None
            }
            (Kind::Ansi, Selection::Unfocused) => None,
            (Kind::Truecolor, Selection::Unfocused) => Some(self.cursor_line),
            (Kind::Truecolor, Selection::Visual) => Some(self.range),
            _ => Some(self.selection),
        };
        if let Some(bg) = bg {
            for x in row.left()..row.right() {
                let cell = &mut buf[(x, row.y)];
                // A pill and an avatar chip keep their background.
                if (cell.bg == self.error && self.error != Color::Reset) || self.chip_bg(cell.bg) {
                    continue;
                }
                cell.bg = bg;
                if cell.fg == bg || cell.fg == self.fg_dim || cell.fg == self.fg_muted || cell.fg == self.border {
                    cell.fg = self.fg;
                }
            }
        }
        let gutter = match (self.kind, how) {
            (Kind::Truecolor, _) | (_, Selection::Focused) => None,
            (_, Selection::Unfocused) => Some(self.muted()),
            (_, Selection::Visual) => Some(self.key()),
        };
        if let Some(style) = gutter {
            let cell = &mut buf[(row.x, row.y)];
            cell.set_symbol(GUTTER);
            cell.set_style(style);
        }
    }

    /// Dim what is drawn in `area`, so a dialog drawn on top of it stands out while the screen
    /// below stays readable. Symbols are kept.
    pub fn dim_area(&self, buf: &mut Buffer, area: Rect) {
        let area = area.intersection(buf.area);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let cell = &mut buf[(x, y)];
                match self.dim {
                    Dim::Blend { .. } => {
                        cell.fg = self.dimmed(cell.fg, self.fg);
                        cell.bg = self.dimmed(cell.bg, self.bg);
                    }
                    Dim::Modifier => {
                        cell.modifier.insert(Modifier::DIM);
                    }
                }
            }
        }
    }

    /// `c` blended toward the dim tone; a terminal default color is treated as `default`.
    fn dimmed(&self, c: Color, default: Color) -> Color {
        let Dim::Blend { toward, keep } = self.dim else { return c };
        let parts = |c: Color| if let Color::Rgb(r, g, b) = c { Some((r, g, b)) } else { None };
        let (Some((r, g, b)), Some((tr, tg, tb))) = (parts(c).or(parts(default)), parts(toward)) else { return c };
        let mix = |a: u8, z: u8| ((u16::from(a) * keep + u16::from(z) * (100 - keep)) / 100) as u8;
        Color::Rgb(mix(r, tr), mix(g, tg), mix(b, tb))
    }
}

#[cfg(test)]
mod tests;
