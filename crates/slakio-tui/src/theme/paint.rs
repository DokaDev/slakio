//! Painting over a row already drawn: the selection bar. A few cells are never painted over
//! ([`Theme::keeps`]): what they show is told by their own background or colour.

use super::{GUTTER, Kind, Selection, Theme};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

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

#[cfg(test)]
mod tests {
    use crate::images::graphics;
    use crate::theme::{BUILTINS, Kind, Selection};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

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
}
