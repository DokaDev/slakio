//! The look of a reaction chip: a quiet pill a little apart from the background, the emoji in
//! body text and the count muted; the user's own on a light wash of the accent, its count in
//! the accent. With 24-bit colour each end of the pill is a rounded cap in the chip's colour.

use super::code::{contrast, mix};
use super::{Kind, Theme};
use ratatui::style::{Color, Modifier, Style};

/// The percent of body text mixed into the background for a reaction chip a theme file leaves
/// out ([`Theme::derive_reactions`]).
pub const REACTION_SHARE: u16 = 10;

/// The least contrast of a chip from the background: seen, never loud.
pub const CHIP_APART: f64 = 1.15;

/// The least a chip's colour differs, on some channel, from the bars and the other chip.
pub const CHANNEL_APART: u8 = 8;

/// The least contrast of the accent count on the user's own chip; under it the count is body
/// text.
const COUNT_ON_MINE: f64 = 3.0;

/// How a reaction chip is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChipLook {
    /// The pad and the emoji (a picture's cells take its background only).
    pub body: Style,
    /// The count.
    pub count: Style,
    /// The rounded ends of the pill (the chip's colour on the row's background), where the chip
    /// has a background of its own.
    pub cap: Option<Style>,
}

/// The cap of a chip of background `bg`: its colour as the glyph's.
fn cap(bg: Option<Color>) -> Option<Style> {
    bg.filter(|c| *c != Color::Reset).map(|c| Style::new().fg(c))
}

/// The largest difference of two RGB colours on one channel (`None` for a terminal colour).
pub(super) fn channel_gap(a: Color, b: Color) -> Option<u8> {
    let (Color::Rgb(r, g, b1), Color::Rgb(r2, g2, b2)) = (a, b) else { return None };
    Some(r.abs_diff(r2).max(g.abs_diff(g2)).max(b1.abs_diff(b2)))
}

impl Theme {
    /// The look of a reaction chip, the user's own when `mine`. A theme file's `reaction` /
    /// `reaction_mine` style is the pad, the emoji and the count alike, its `bg` the cap's
    /// colour. Without a chip colour (the 16-color theme, no colour at all): muted text, the
    /// user's own in the accent and bold, no cap.
    pub fn reaction_chip(&self, mine: bool) -> ChipLook {
        let set = if mine { self.styles.reaction_mine } else { self.styles.reaction };
        if let Some(s) = set {
            return ChipLook { body: s, count: s, cap: cap(s.bg) };
        }
        let chip = if mine { self.reaction_mine } else { self.reaction };
        if self.kind != Kind::Truecolor || chip == Color::Reset {
            let body = if mine { self.muted().fg(self.accent).add_modifier(Modifier::BOLD) } else { self.muted() };
            return ChipLook { body, count: body, cap: None };
        }
        let body = self.text().bg(chip);
        let count = match mine {
            false => self.muted().bg(chip),
            true if contrast(self.accent, chip).is_some_and(|k| k < COUNT_ON_MINE) => body.add_modifier(Modifier::BOLD),
            true => self.accent().bg(chip).add_modifier(Modifier::BOLD),
        };
        ChipLook { body, count, cap: cap(Some(chip)) }
    }

    /// `c` is the background of a reaction chip (kept under a bar painted over its row), and of
    /// nothing else a bar paints over: a surface or a bar that happens to be a chip's colour is
    /// still painted.
    pub(super) fn reaction_bg(&self, c: Color) -> bool {
        let bars = [self.surface, self.surface_alt, self.selection, self.cursor_line, self.range, self.hover];
        ![Color::Reset, self.bg].into_iter().chain(bars).any(|b| b == c)
            && [false, true].into_iter().any(|mine| self.reaction_chip(mine).body.bg == Some(c))
    }

    /// The chip colours a theme file leaves out, worked out from the colours it sets: body text
    /// [`REACTION_SHARE`] percent into the background where it sets `bg` or `fg`, and the least
    /// share of the accent (from 10 percent) [`CHIP_APART`] from the background and
    /// [`CHANNEL_APART`] from the selection, the hover colour and the other chip where it sets
    /// `bg` or `accent`; neither ever code's tint. A colour that is not RGB keeps the base theme's chip.
    pub(super) fn derive_reactions(&mut self, set: impl Fn(&str) -> bool) {
        // Never code's tint: a bar painted over a row keeps a chip, and tints code apart.
        let code = self.code_bg();
        if !set("reaction")
            && (set("bg") || set("fg"))
            && let Some(c) = [REACTION_SHARE, REACTION_SHARE - 1, REACTION_SHARE + 1]
                .into_iter()
                .filter_map(|p| mix(self.fg, self.bg, p))
                .find(|c| *c != code)
        {
            self.reaction = c;
        }
        if !set("reaction_mine") && (set("bg") || set("accent")) {
            let apart = |c: Color, from: Color| channel_gap(c, from).is_none_or(|d| d >= CHANNEL_APART);
            let wash = (10..=40).filter_map(|p| mix(self.accent, self.bg, p)).find(|c| {
                contrast(*c, self.bg).is_some_and(|k| k >= CHIP_APART)
                    && *c != code
                    && apart(*c, self.selection)
                    && apart(*c, self.reaction)
                    && apart(*c, self.hover)
            });
            if let Some(c) = wash.or_else(|| mix(self.accent, self.bg, 40)) {
                self.reaction_mine = c;
            }
        }
    }
}
