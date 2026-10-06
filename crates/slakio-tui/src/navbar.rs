//! Navigation in the list panel, laid out once for drawing ([`crate::ui`]) and the mouse, so a
//! click lands on exactly what was drawn there: the panel's title is the workspace chip (its
//! color band, its name, `▾`, then each other workspace that wants attention by its letter and
//! mark), the panel's first rows the views, one row each as in GUI Slack's sidebar, with their
//! counts at the right (ui-ux-spec: `@n` mentions, `●n` a view's unread messages, `●` unread
//! channels).
//!
//! ```text
//! ╭ ▌A company ▾ · B @9 ──────╮
//! │ 󰋜 Home                @24 │    icons on
//! │ 󰍡 DMs                 ●11 │
//! │ 󰂚 Activity            @37 │
//! │ 󰈙 Files                   │
//! │ 󰃀 Later                   │
//! ├───────────────────────────┤
//!
//! │ ▸ 󰂚 Activity          @37 │    folded (`nav_rows = "collapsed"`): the view shown alone
//! │ Activity              @37 │    icons off: the names alone
//! ```
//!
//! A Nerd Font glyph is drawn as one cell followed by a blank one, and both cells are its slot
//! ([`GLYPH_SLOT`]): a terminal that draws the glyph two cells wide (Ghostty does when the next
//! cell is blank; a terminal set to treat ambiguous characters as wide always does) never
//! covers the text after it.
//!
//! A count is never cut or dropped: when a row is short, the view's name is cut (`…`). The
//! chip's name is cut first, then its marks lose their numbers, then the name goes down to its
//! letter.

use crate::text::{clip, width};
use ratatui::layout::Rect;
use unicode_segmentation::UnicodeSegmentation;

/// The cells a Nerd Font glyph takes: itself and a blank one after it.
pub const GLYPH_SLOT: u16 = 2;

/// The workspace chip: the name of the workspace shown, then, after `▾`, each other workspace
/// that wants attention by its letter and its mark (` · B @9`): never a count beside the name
/// shown, which is not its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chip {
    pub name: String,
    /// The other workspaces with something unread: (letter, mark).
    pub others: Vec<(String, String)>,
}

/// A view's row: its glyph (icons on), its name, its count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub glyph: Option<&'static str>,
    pub label: String,
    pub badge: Option<String>,
}

/// A part of a line as drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The workspace's color band.
    Band,
    Name,
    /// `▾`.
    Caret,
    /// Another workspace's letter, after `▾`.
    Other,
    /// That workspace's mark (`@9`, `●`).
    Mark,
    /// `▸`: the views are folded to the one shown.
    Fold,
    /// A Nerd Font glyph: one cell, then a blank one ([`GLYPH_SLOT`]).
    Glyph,
    Label,
    Badge,
    Blank,
}

/// A stretch of a line: whose (the view's index into the views; `None` on the chip, which is
/// one thing), which part, from column `x`, `width` cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub item: Option<usize>,
    pub part: Part,
    pub x: u16,
    pub text: String,
    pub width: u16,
}

/// A line laid out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bar {
    pub area: Rect,
    pub pieces: Vec<Piece>,
}

impl Bar {
    /// The view at column `x` of a view's row.
    pub fn hit(&self, x: u16) -> Option<usize> {
        self.pieces.iter().find(|p| x >= p.x && x < p.x + p.width).and_then(|p| p.item)
    }

    /// The columns the line covers.
    pub fn extent(&self) -> (u16, u16) {
        let from = self.pieces.first().map_or(self.area.x, |p| p.x);
        (from, self.pieces.last().map_or(from, |p| p.x + p.width))
    }
}

/// A count without its number (`@3` → `@`).
fn compact(badge: &str) -> String {
    badge.chars().take(1).collect()
}

/// The name cut to `cells`: with `…` (never after a space), or its first letter alone when
/// fewer than four cells are left for it.
fn cut_name(name: &str, cells: usize) -> String {
    if cells < 4 {
        return name.graphemes(true).next().unwrap_or_default().to_string();
    }
    let cut = clip(name, cells);
    match cut.strip_suffix('…') {
        Some(kept) => format!("{}…", kept.trim_end()),
        None => cut,
    }
}

type Parts = Vec<(Option<usize>, Part, String, u16)>;

fn push(out: &mut Parts, item: Option<usize>, part: Part, text: String) {
    let w = if part == Part::Glyph { GLYPH_SLOT } else { width(&text) as u16 };
    out.push((item, part, text, w));
}

fn total(p: &Parts) -> usize {
    p.iter().map(|(.., w)| usize::from(*w)).sum()
}

/// Lay `parts` out from the left of `area`, cut at its end: a glyph that does not fit whole is
/// left out.
fn place(area: Rect, parts: Parts) -> Bar {
    let mut bar = Bar { area, pieces: Vec::new() };
    let mut x = area.x;
    let end = area.x + area.width;
    for (item, part, text, w) in parts {
        if x >= end {
            break;
        }
        let left = end - x;
        if part == Part::Glyph && w > left {
            break;
        }
        let (text, w) = if part == Part::Glyph || w <= left {
            (text, w)
        } else {
            let t = clip(&text, usize::from(left));
            let w = width(&t) as u16;
            (t, w)
        };
        bar.pieces.push(Piece { item, part, x, text, width: w });
        x += w;
    }
    bar
}

/// The chip's parts with its name `name` and its marks numbered or not.
fn chip_parts(chip: &Chip, name: &str, numbers: bool) -> Parts {
    let mut out = Parts::new();
    push(&mut out, None, Part::Blank, " ".into());
    push(&mut out, None, Part::Band, "▌".into());
    push(&mut out, None, Part::Name, name.to_string());
    push(&mut out, None, Part::Caret, " ▾".into());
    for (letter, m) in &chip.others {
        let m = if numbers { m.clone() } else { compact(m) };
        push(&mut out, None, Part::Blank, " · ".into());
        push(&mut out, None, Part::Other, letter.clone());
        push(&mut out, None, Part::Mark, format!(" {m}"));
    }
    push(&mut out, None, Part::Blank, " ".into());
    out
}

/// Lay the workspace chip out in `area` (the list panel's title row, inside its corners): the
/// name cut first, then the marks lose their numbers, then the name down to its letter.
pub fn chip(area: Rect, chip: &Chip) -> Bar {
    let room = usize::from(area.width);
    let full = width(&chip.name);
    let over = |numbers: bool| total(&chip_parts(chip, &chip.name, numbers)).saturating_sub(room);
    let (numbers, cut) = match (over(true), over(false)) {
        (o, _) if o == 0 || full.saturating_sub(o) >= 8.min(full) => (true, o),
        (_, o) => (false, o),
    };
    let name = if cut == 0 { chip.name.clone() } else { cut_name(&chip.name, full.saturating_sub(cut)) };
    place(area, chip_parts(chip, &name, numbers))
}

/// One view's row in `row` (the row of the list panel it takes): a blank, `▸` before the view
/// when the rows are folded to it (`fold`), its glyph's slot (icons on), its name, and its count
/// at the right, one cell in from the edge. The count is never cut: the name is.
fn view_row(row: Rect, item: usize, v: &Item, fold: bool) -> Bar {
    let mut out = Parts::new();
    let it = Some(item);
    push(&mut out, it, Part::Blank, " ".into());
    if fold {
        push(&mut out, it, Part::Fold, "▸".into());
        push(&mut out, it, Part::Blank, " ".into());
    }
    if let Some(g) = v.glyph {
        push(&mut out, it, Part::Glyph, g.to_string());
    }
    let badge = v.badge.as_deref().map_or(0, width);
    // The name, a blank before the count (when there is one) and the blank after it.
    let room = usize::from(row.width).saturating_sub(total(&out) + badge + usize::from(badge > 0) + 1);
    let label = clip(&v.label, room);
    let fill = room.saturating_sub(width(&label)) + usize::from(badge > 0);
    push(&mut out, it, Part::Label, label);
    push(&mut out, it, Part::Blank, " ".repeat(fill));
    if let Some(b) = &v.badge {
        push(&mut out, it, Part::Badge, b.clone());
    }
    push(&mut out, it, Part::Blank, " ".into());
    place(row, out)
}

/// Lay the views out in `area` (the top of the list panel), one row each, `shown` the view the
/// list shows; `folded`, a single row for the view shown (`▸`). A row is one view from edge to
/// edge (a click anywhere on it is that view's); rows that do not fit in `area` are left out.
pub fn views(area: Rect, views: &[Item], shown: usize, folded: bool) -> Vec<Bar> {
    let row = |i: u16| Rect { y: area.y + i, height: 1, ..area };
    if folded {
        return views
            .get(shown)
            .filter(|_| area.height > 0)
            .map(|v| view_row(row(0), shown, v, true))
            .into_iter()
            .collect();
    }
    views.iter().enumerate().take(usize::from(area.height)).map(|(i, v)| view_row(row(i as u16), i, v, false)).collect()
}

#[cfg(test)]
mod tests;
