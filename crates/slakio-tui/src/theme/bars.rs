//! The styles of the two bars: the top bar (the app's: the workspace and the views, on the status
//! line's surface, a reversed row without truecolor) and the tab bar (the work area's: on the
//! background), and what is shown among their peers, raised alike in both.

use super::{Kind, Theme};
use ratatui::style::{Color, Modifier, Style};

impl Theme {
    /// The tab bar's row, under the tabs.
    pub fn tab_bar(&self) -> Style {
        self.base()
    }

    /// A tab's title (and the spaces of its label): the tab shown on the raised surface, in
    /// bold; the others muted.
    pub fn tab(&self, shown: bool) -> Style {
        match (self.kind, shown) {
            (Kind::Truecolor, true) => self.bold().bg(self.raised()),
            (_, true) => Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            (Kind::Truecolor | Kind::Ansi, false) => self.muted().bg(self.bg),
            (_, false) => Style::new(),
        }
    }

    /// The background of what is shown among its peers (the tab shown, the view shown): the
    /// unfocused selection's on a dark theme, unless the top bar's surface or the background is
    /// that color already; the selection's on a light one (its unfocused one is too faint there).
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

    /// A tab's unread count (`●3`): a mention's color when one mentions the user.
    pub fn tab_badge(&self, shown: bool, mention: bool) -> Style {
        let dot = self.dot(mention);
        match self.tab(shown).bg {
            Some(bg) => dot.bg(bg),
            None => dot,
        }
    }

    /// The top bar's row: the status line's surface; without truecolor the whole row reversed,
    /// so it reads apart from the tab bar by shape, not only color.
    pub fn nav_bar(&self) -> Style {
        match self.kind {
            Kind::Truecolor => self.text().bg(self.surface),
            _ => Style::new().add_modifier(Modifier::REVERSED),
        }
    }

    /// A view on the top bar: the one the list shows raised and bold like the tab shown (out of
    /// the reversed bar without truecolor), the others muted.
    pub fn nav_item(&self, shown: bool) -> Style {
        match (self.kind, shown) {
            (Kind::Truecolor, true) => self.tab(true),
            (Kind::Truecolor, false) => self.muted().bg(self.surface),
            (_, true) => Style::new().add_modifier(Modifier::BOLD).remove_modifier(Modifier::REVERSED),
            (_, false) => self.nav_bar(),
        }
    }

    /// A count on the top bar: a mention in the mention color (out of the reversed bar without
    /// truecolor), unread bold.
    pub fn nav_marker(&self, mention: bool) -> Style {
        match (self.kind, mention) {
            (Kind::Truecolor, m) => self.dot(m).bg(self.surface),
            // Inside the reversed row: a mention in its color there, unread bold.
            (_, true) => self.nav_bar().patch(self.dot(true)),
            (_, false) => self.nav_bar().add_modifier(Modifier::BOLD),
        }
    }

    /// The count of the view shown: its marker on the raised background (out of the reversed
    /// bar without truecolor).
    pub fn nav_shown_marker(&self, mention: bool) -> Style {
        match self.kind {
            Kind::Truecolor => self.dot(mention).bg(self.raised()),
            _ => self.dot(mention).remove_modifier(Modifier::REVERSED),
        }
    }

    /// The top bar's quiet marks (`▾`, the separator): muted on its surface.
    pub fn nav_quiet(&self) -> Style {
        match self.kind {
            Kind::Truecolor => self.muted().bg(self.surface),
            _ => self.nav_bar(),
        }
    }

    /// A count on the top bar (`@3`, `●2`, `●`): a mention in the mention color, unread bold;
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
