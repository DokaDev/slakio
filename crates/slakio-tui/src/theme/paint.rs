//! Painting over a row already drawn: the selection bar, and the quieter wash of the row under
//! the mouse. A few cells are never painted over ([`Theme::keeps`]): what they show is told by
//! their own background or colour.

use super::code::{contrast, mix};
use super::reactions::{CHANNEL_APART, channel_gap};
use super::{GUTTER, Kind, Selection, Theme};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

/// The gutter mark of the row under the mouse where the theme has no hover colour: thinner than
/// a selection's ([`GUTTER`]).
pub const HOVER_GUTTER: &str = "\u{258F}";

/// The percent of the accent mixed into the background for the hover colour a theme file leaves
/// out ([`Theme::derive_hover`]).
pub const HOVER_SHARE: u16 = 9;

/// The least contrast of the hover colour from the background: seen, quieter than the selection.
pub const HOVER_APART: f64 = 1.10;

impl Theme {
    /// `c` is the background of an avatar chip (kept under a selection bar), and of nothing
    /// else a bar paints over: a surface that happens to be a chip's color is still a surface.
    fn chip_bg(&self, c: Color) -> bool {
        ![Color::Reset, self.bg, self.surface, self.surface_alt, self.selection].contains(&c)
            && ((0..self.avatars.len()).any(|i| self.avatar(i).bg == Some(c)) || self.avatar_group().bg == Some(c))
    }

    /// `cell` stays as it is under anything painted over its row: a pill (a background of the
    /// error color), an avatar chip, a reaction chip, and a picture's cell (its colour says
    /// which picture).
    pub(super) fn keeps(&self, cell: &Cell) -> bool {
        (cell.bg == self.error && self.error != Color::Reset)
            || self.chip_bg(cell.bg)
            || self.reaction_bg(cell.bg)
            || crate::images::graphics::is_placeholder(cell.symbol())
    }

    /// Paint `how` over the row `row` already drawn, whatever it is drawn on (the app's
    /// background, a popup's surface). Its first cell is the gutter: where the theme has no
    /// background for `how`, a bar there marks the row. Muted and faint text, and text the bar
    /// would hide, take the body color so it stays readable on the bar; bold, and the cells the
    /// theme [keeps](Self::keeps), stay.
    pub fn paint_selection(&self, buf: &mut Buffer, row: Rect, how: Selection) {
        let row = row.intersection(buf.area);
        if row.is_empty() {
            return;
        }
        let bg = match (self.kind, how) {
            (Kind::NoColor, Selection::Unfocused) => None,
            (Kind::NoColor, _) => {
                for x in row.left()..row.right() {
                    if !crate::images::graphics::is_placeholder(buf[(x, row.y)].symbol()) {
                        buf[(x, row.y)].modifier.insert(Modifier::REVERSED);
                    }
                }
                None
            }
            (Kind::Ansi, Selection::Unfocused) => None,
            // The terminal's own colour (a file on `terminal` that names a `#rrggbb`) shows no
            // bar: the gutter marks the row instead.
            (Kind::Truecolor, Selection::Unfocused) => Some(self.cursor_line).filter(|c| *c != Color::Reset),
            (Kind::Truecolor, Selection::Visual) => Some(self.range),
            _ => Some(self.selection),
        };
        let bg = self.selection_style(how).and_then(|s| s.bg).or(bg);
        if let Some(bg) = bg {
            // Code keeps a tint of its own, apart from the bar.
            let code = (self.kind == Kind::Truecolor).then(|| (self.code_bg(), self.code_on(bg)));
            for x in row.left()..row.right() {
                if self.keeps(&buf[(x, row.y)]) {
                    continue;
                }
                let cell = &mut buf[(x, row.y)];
                cell.bg = match code {
                    Some((on_bg, on_bar)) if cell.bg == on_bg => on_bar,
                    _ => bg,
                };
                if cell.fg == bg || cell.fg == self.fg_dim || cell.fg == self.fg_muted || cell.fg == self.border {
                    cell.fg = self.fg;
                }
            }
        }
        self.paint_over(buf, row, how);
        let gutter = match (self.kind, how) {
            (Kind::Truecolor, Selection::Visual) | (_, Selection::Focused) => None,
            (_, Selection::Unfocused) if bg.is_some() => None,
            (_, Selection::Unfocused) => Some(self.muted()),
            (_, Selection::Visual) => Some(self.key()),
        };
        if let Some(style) = gutter {
            let cell = &mut buf[(row.x, row.y)];
            cell.set_symbol(GUTTER);
            cell.set_style(style);
        }
    }
}

impl Theme {
    /// Paint the row under the mouse, `row`, already drawn: its cells take the hover colour (code
    /// its own tint on it), their text as it is; the cells the theme [keeps](Self::keeps) stay.
    /// Without a hover colour (the 16-color theme, no colour at all) a thin bar in the gutter, the
    /// row's first cell, marks it, and only where that cell is blank.
    pub fn paint_hover(&self, buf: &mut Buffer, row: Rect) {
        let row = row.intersection(buf.area);
        if row.is_empty() {
            return;
        }
        if self.kind == Kind::NoColor || self.hover == Color::Reset {
            let cell = &mut buf[(row.x, row.y)];
            if cell.symbol() == " " {
                cell.set_symbol(HOVER_GUTTER);
                if self.kind != Kind::NoColor {
                    cell.set_style(Style::new().fg(self.fg_dim));
                }
            }
            return;
        }
        let code = (self.kind == Kind::Truecolor).then(|| (self.code_bg(), self.code_on(self.hover)));
        for x in row.left()..row.right() {
            if self.keeps(&buf[(x, row.y)]) {
                continue;
            }
            let cell = &mut buf[(x, row.y)];
            cell.bg = match code {
                Some((on_bg, on_hover)) if cell.bg == on_bg => on_hover,
                _ => self.hover,
            };
        }
    }

    /// The hover colour a theme file leaves out, where it sets `bg` or `accent`: the least share
    /// of the accent (from [`HOVER_SHARE`] percent) into the background [`HOVER_APART`] from it,
    /// nearer it than the selection, body text 4.5:1 on it, and [`CHANNEL_APART`] from the bars,
    /// the chips, the surface and code's tint. A colour that is not RGB keeps the base's.
    pub(super) fn derive_hover(&mut self, set: impl Fn(&str) -> bool) {
        if set("hover") || !(set("bg") || set("accent")) {
            return;
        }
        let code = self.code_bg();
        let others =
            [self.selection, self.cursor_line, self.range, self.surface, self.reaction, self.reaction_mine, code];
        let quiet = |c: Color| {
            let apart = contrast(c, self.bg);
            apart.is_some_and(|k| k >= HOVER_APART)
                && contrast(self.selection, self.bg).is_none_or(|s| apart.is_some_and(|k| k < s))
                && contrast(self.fg, c).is_some_and(|k| k >= 4.5)
                && others.iter().all(|o| channel_gap(c, *o).is_none_or(|d| d >= CHANNEL_APART))
        };
        let wash = (HOVER_SHARE..=30).filter_map(|p| mix(self.accent, self.bg, p)).find(|c| quiet(*c));
        if let Some(c) = wash.or_else(|| mix(self.accent, self.bg, HOVER_SHARE)) {
            self.hover = c;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HOVER_GUTTER;
    use crate::images::graphics;
    use crate::theme::{BUILTINS, Background, GUTTER, Kind, Selection, Theme, resolve};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Style};

    /// A row of text, then a reaction chip of each kind (cap, emoji, count, cap) and a picture's
    /// cells: a bar painted over it takes the text and the caps' row, never the chips or the
    /// picture.
    #[test]
    fn selection_keeps_reaction_chip_cells() {
        for t in BUILTINS.iter().filter(|t| t.kind == Kind::Truecolor) {
            for how in [Selection::Focused, Selection::Unfocused, Selection::Visual] {
                let area = Rect::new(0, 0, 24, 1);
                let mut buf = Buffer::empty(area);
                buf.set_style(area, t.base());
                buf.set_string(0, 0, "ok", t.text());
                let mut chips = Vec::new();
                for (x, mine) in [(3, false), (10, true)] {
                    let look = t.reaction_chip(mine);
                    let cap = look.cap.expect("a 24-bit chip has caps");
                    buf.set_string(x, 0, "\u{E0B6}", cap);
                    buf.set_string(x + 1, 0, "ab", look.body);
                    buf.set_string(x + 3, 0, " 2", look.count);
                    buf.set_string(x + 5, 0, "\u{E0B4}", cap);
                    chips.extend((x + 1..x + 5).map(|x| (x, buf[(x, 0)].clone())));
                }
                let picture = Style::new().fg(graphics::color(0x0012_3456)).bg(t.reaction);
                buf.set_string(18, 0, graphics::row(0, 2), picture);
                let pictured: Vec<_> = (18..20).map(|x| buf[(x, 0)].clone()).collect();
                t.paint_selection(&mut buf, area, how);
                let n = &t.name;
                for (x, before) in &chips {
                    assert_eq!(&buf[(*x, 0)], before, "{n} {how:?}: chip cell {x}");
                }
                assert_eq!((18..20).map(|x| buf[(x, 0)].clone()).collect::<Vec<_>>(), pictured, "{n} {how:?}");
                let bar = buf[(1, 0)].bg;
                assert_ne!(bar, t.bg, "{n} {how:?}: the text takes the bar");
                for (x, chip) in [(3, t.reaction), (8, t.reaction), (10, t.reaction_mine), (15, t.reaction_mine)] {
                    assert_eq!((buf[(x, 0)].fg, buf[(x, 0)].bg), (chip, bar), "{n} {how:?}: cap {x} on the bar");
                }
            }
        }
    }

    /// A row of text, a pill, an avatar chip, a reaction chip, code and a picture's cells: the
    /// hover takes exactly the text's cells and the blanks (code its own tint on the hover), their
    /// text as it was, and leaves every other cell as it is.
    #[test]
    fn hover_paints_exact_cells_and_skips_pictures_pills_and_chips() {
        for t in BUILTINS.iter().filter(|t| t.kind == Kind::Truecolor) {
            let area = Rect::new(0, 0, 30, 1);
            let mut buf = Buffer::empty(area);
            buf.set_style(area, t.base());
            buf.set_string(1, 0, "ok", t.text());
            buf.set_string(4, 0, " 3 ", t.badge());
            buf.set_string(8, 0, "AB", t.avatar(0));
            buf.set_string(11, 0, " ab 2 ", t.reaction_chip(false).body);
            buf.set_string(18, 0, "x=1", t.code());
            let picture = Style::new().fg(graphics::color(0x0012_3456));
            buf.set_string(23, 0, graphics::row(0, 2), picture);
            let before = buf.clone();
            t.paint_hover(&mut buf, area);
            let n = &t.name;
            let kept =
                |x: u16| (4..7).contains(&x) || (8..10).contains(&x) || (11..17).contains(&x) || (23..25).contains(&x);
            for x in 0..30 {
                let (b, a) = (&before[(x, 0)], &buf[(x, 0)]);
                if kept(x) {
                    assert_eq!(a, b, "{n}: cell {x} is kept");
                    continue;
                }
                let bg = if (18..21).contains(&x) { t.code_on(t.hover) } else { t.hover };
                assert_eq!((a.symbol(), a.fg, a.bg, a.modifier), (b.symbol(), b.fg, bg, b.modifier), "{n}: cell {x}");
            }
            assert_ne!(t.code_on(t.hover), t.hover, "{n}: code apart from the hover");
        }
    }

    /// Without a hover colour the row's blank gutter cell takes a thin bar (faint on 16 colours,
    /// plain without colour), never a cell with something in it, and never the selection's bar;
    /// nothing else of the row changes.
    #[test]
    fn hover_without_a_background_is_a_thin_gutter_apart_from_the_selection_bar() {
        assert_ne!(HOVER_GUTTER, GUTTER);
        for t in [resolve("terminal", false, Background::Dark), Theme::no_color()] {
            let n = &t.name;
            let area = Rect::new(0, 0, 8, 1);
            let mut buf = Buffer::empty(area);
            buf.set_string(1, 0, "ok", t.text());
            let before = buf.clone();
            t.paint_hover(&mut buf, area);
            let gutter = &buf[(0, 0)];
            assert_eq!(gutter.symbol(), HOVER_GUTTER, "{n}");
            let fg = if t.kind == Kind::NoColor { Color::Reset } else { t.fg_dim };
            assert_eq!((gutter.fg, gutter.bg, gutter.modifier), (fg, Color::Reset, before[(0, 0)].modifier), "{n}");
            for x in 1..8 {
                assert_eq!(buf[(x, 0)], before[(x, 0)], "{n}: cell {x}");
            }
            // The unfocused selection's bar is the wider mark, and stays under a hover.
            let mut buf = Buffer::empty(area);
            t.paint_selection(&mut buf, area, Selection::Unfocused);
            assert_eq!(buf[(0, 0)].symbol(), GUTTER, "{n}");
            let selected = buf.clone();
            t.paint_hover(&mut buf, area);
            assert_eq!(buf, selected, "{n}: no gutter over the selection's bar");
            // A row whose first cell holds something: no gutter.
            let mut buf = Buffer::empty(area);
            buf.set_string(0, 0, "x", t.text());
            let before = buf.clone();
            t.paint_hover(&mut buf, area);
            assert_eq!(buf, before, "{n}: a non-blank first cell");
        }
    }

    /// A chip whose colour is a surface or a bar (a theme file's `reaction` or `reaction_mine`
    /// set to one of them) keeps no row drawn on that colour: the selection and the hover paint
    /// every cell of it, and a chip of the other kind is still kept.
    #[test]
    fn a_chip_the_colour_of_a_surface_or_a_bar_keeps_no_row_of_it() {
        for base in BUILTINS.iter().filter(|t| t.kind == Kind::Truecolor) {
            let bars = [base.surface, base.surface_alt, base.selection, base.cursor_line, base.range, base.hover];
            for (mine, c) in [false, true].into_iter().flat_map(|m| bars.map(|c| (m, c))) {
                let mut t = Theme::clone(base);
                *(if mine { &mut t.reaction_mine } else { &mut t.reaction }) = c;
                let n = format!("{} mine={mine} {c:?}", t.name);
                let area = Rect::new(0, 0, 10, 1);
                for (how, want) in [(Some(Selection::Focused), t.selection), (None, t.hover)] {
                    let mut buf = Buffer::empty(area);
                    buf.set_style(area, Style::new().fg(t.fg).bg(c));
                    match how {
                        Some(how) => t.paint_selection(&mut buf, area, how),
                        None => t.paint_hover(&mut buf, area),
                    }
                    let painted = (0..10).filter(|x| buf[(*x, 0)].bg == want).count();
                    assert_eq!(painted, 10, "{n} {how:?}: the row on the chip's colour is painted");
                }
                // The other kind of chip, its colour none of them, is still kept.
                let other = t.reaction_chip(!mine).body;
                let mut buf = Buffer::empty(area);
                buf.set_style(area, t.base());
                buf.set_string(2, 0, "ab", other);
                let chip = buf[(2, 0)].clone();
                t.paint_selection(&mut buf, area, Selection::Focused);
                t.paint_hover(&mut buf, area);
                assert_eq!(buf[(2, 0)], chip, "{n}: the other chip is kept");
            }
        }
    }
}
