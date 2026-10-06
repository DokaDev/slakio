//! Navigation in the list panel, laid out once for drawing ([`crate::ui`]) and the mouse, so a
//! click lands on exactly what was drawn there: the panel's title is the workspace chip (its
//! color band, its name, `▾`, then each other workspace that wants attention by its letter and
//! mark), the panel's first row the views with their counts (ui-ux-spec: `@n` mentions, `●n`
//! a view's unread messages, `●` unread channels). Only the view shown spells its name.
//!
//! ```text
//! ╭ ▌A company ▾ · B @9 ──────╮
//! │ 󰋜 Home  󰍡 ●11  󰂚 @37  󰈙  󰃀 │    icons on
//! │ Home  D●11  A@37  F  L     │    icons off
//! ├────────────────────────────┤
//! ```
//!
//! A Nerd Font glyph is drawn as one cell followed by a blank one, and both cells are its slot
//! ([`GLYPH_SLOT`]): a terminal that draws the glyph two cells wide (Ghostty does when the next
//! cell is blank; a terminal set to treat ambiguous characters as wide always does) never
//! covers the text after it, and a click on either cell is the glyph's.
//!
//! When the switcher does not fit, the views go one cell apart instead of two, then their counts
//! lose their numbers (`@`, `●`), then the row is cut (never inside a glyph). The chip's name is
//! cut first, then its marks lose their numbers, then the name goes down to its letter.

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

/// A view on the switcher row: its glyph (icons on), its name, its letter, its count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub glyph: Option<&'static str>,
    pub label: String,
    pub short: String,
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
    /// A Nerd Font glyph: one cell, then a blank one ([`GLYPH_SLOT`]).
    Glyph,
    Label,
    Badge,
    Blank,
}

/// A stretch of a line: whose (the view's index on the switcher row; `None` on the chip, which
/// is one thing), which part, from column `x`, `width` cells.
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
    /// The view at column `x` of the switcher row.
    pub fn hit(&self, x: u16) -> Option<usize> {
        self.pieces.iter().find(|p| x >= p.x && x < p.x + p.width).and_then(|p| p.item)
    }

    /// The columns view `item` covers, `[from, to)`.
    pub fn span(&self, item: usize) -> Option<(u16, u16)> {
        let mine = self.pieces.iter().filter(|p| p.item == Some(item));
        let from = mine.clone().map(|p| p.x).min()?;
        Some((from, mine.map(|p| p.x + p.width).max()?))
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

/// The views' parts, `shown` the view the list shows (its name spelled), counts numbered or
/// not, `gap` blank cells between two views (a glyph's slot ends with one of them).
fn view_parts(views: &[Item], shown: usize, numbers: bool, gap: usize) -> Parts {
    let mut out = Parts::new();
    push(&mut out, None, Part::Blank, " ".into());
    for (i, v) in views.iter().enumerate() {
        let item = Some(i);
        if i > 0 {
            let bare = out.last().is_some_and(|p| p.1 == Part::Glyph);
            let n = if bare { gap - 1 } else { gap };
            if n > 0 {
                push(&mut out, None, Part::Blank, " ".repeat(n));
            }
        }
        if let Some(g) = v.glyph {
            push(&mut out, item, Part::Glyph, g.to_string());
        }
        let text = if i == shown { Some(&v.label) } else { v.glyph.is_none().then_some(&v.short) };
        if let Some(t) = text {
            push(&mut out, item, Part::Label, t.clone());
        }
        if let Some(b) = &v.badge {
            let b = if numbers { b.clone() } else { compact(b) };
            // Apart from a name; right after a letter or a glyph's slot (`D●2`, `󰍡 ●2`).
            let b = if i == shown { format!(" {b}") } else { b };
            push(&mut out, item, Part::Badge, b);
        }
    }
    out
}

/// Lay the view switcher out in `area` (the list panel's first row): two cells between the
/// views, then one, then counts without numbers, then cut.
pub fn views(area: Rect, views: &[Item], shown: usize) -> Bar {
    let room = usize::from(area.width);
    let (numbers, gap) = [(true, 2), (true, 1), (false, 1)]
        .into_iter()
        .find(|&(n, g)| total(&view_parts(views, shown, n, g)) <= room)
        .unwrap_or((false, 1));
    place(area, view_parts(views, shown, numbers, gap))
}

#[cfg(test)]
mod tests;
