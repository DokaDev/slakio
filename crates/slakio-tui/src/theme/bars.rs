//! The styles of the two rows of peers: the tab bar (the work area's) and the view switcher (the
//! list panel's first row), both on the background, and what is shown among the peers, raised
//! alike in both.

use super::{Kind, Theme};
use ratatui::style::{Color, Modifier, Style};

impl Theme {
    /// The tab bar's row, under the tabs.
    pub fn tab_bar(&self) -> Style {
        self.base()
    }

    /// A tab's title (and the spaces of its label), or a view on the view switcher: the one shown
    /// on the raised surface, in bold; the others muted.
    pub fn tab(&self, shown: bool) -> Style {
        match (self.kind, shown) {
            (Kind::Truecolor, true) => self.bold().bg(self.raised()),
            (_, true) => Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            (Kind::Truecolor | Kind::Ansi, false) => self.muted().bg(self.bg),
            (_, false) => Style::new(),
        }
    }

    /// The background of what is shown among its peers (the tab shown, the view shown): the
    /// unfocused selection's on a dark theme, unless the status line's surface or the background
    /// is that color already; the selection's on a light one (its unfocused one is too faint there).
    pub fn raised(&self) -> Color {
        let light = luminance(self.bg).is_some_and(|l| l > 0.5);
        if light || self.cursor_line == self.surface || self.cursor_line == self.bg {
            self.selection
        } else {
            self.cursor_line
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

    /// A tab's or a view's unread count (`●3`, `@3`): a mention's color when one mentions the
    /// user.
    pub fn tab_badge(&self, shown: bool, mention: bool) -> Style {
        let dot = self.dot(mention);
        match self.tab(shown).bg {
            Some(bg) => dot.bg(bg),
            None => dot,
        }
    }

    /// A count in the workspace switcher (`@3`, `●2`, `●`): a mention in the mention color, unread bold;
    /// without truecolor a mention is reversed too, so it never reads by color alone.
    pub fn marker(&self, mention: bool) -> Style {
        match (mention, self.kind) {
            (true, Kind::Truecolor) => self.dot(true),
            (true, _) => self.dot(true).add_modifier(Modifier::REVERSED),
            (false, _) => self.bold(),
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
}

/// The relative luminance of an RGB color (`None` for a terminal color).
fn luminance(c: Color) -> Option<f64> {
    let Color::Rgb(r, g, b) = c else { return None };
    let lin = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.039_28 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    Some(0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b))
}
