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
    /// error color), an avatar chip, and a picture's cell (its colour says which picture).
    pub(super) fn keeps(&self, cell: &Cell) -> bool {
        (cell.bg == self.error && self.error != Color::Reset)
            || self.chip_bg(cell.bg)
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
