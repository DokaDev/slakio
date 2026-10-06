//! The top bar's geometry, shared by drawing ([`crate::ui`]) and the mouse, so a click lands on
//! exactly what was drawn there. The bar is the screen's first row: the workspace shown as a
//! chip (its color band, its name, `▾`, then each other workspace that wants attention by its
//! letter and mark), then the views, each with its count (ui-ux-spec: `@n` mentions, `●n` a
//! view's unread messages, `●` unread channels).
//!
//! ```text
//!  ▌A company ▾ · B @9 │ 󰋜  Home @24  󰍡  DMs ●16  󰂚  Activity @37  󰈙  Files  󰃀  Later
//!  ▌A company ▾ · B @9 │ Home @24  DMs ●16  Activity @37  Files  Later          (icons off)
//! ```
//!
//! A Nerd Font glyph is drawn as one cell followed by a blank one, and both cells are its slot
//! ([`GLYPH_SLOT`]): a terminal that draws the glyph two cells wide (Ghostty does when the next
//! cell is blank; a terminal set to treat ambiguous characters as wide always does) never covers
//! the text after it, and a click on either cell is the glyph's.
//!
//! When the bar does not fit, in turn: the workspace's name is cut (eight cells kept), the
//! views drop their glyphs (words read better), then their names for the glyphs (or letters,
//! without icons), the counts lose their numbers (`@`, `●`), the name is cut down to its first
//! letter. It never wraps.

use crate::text::{clip, width};
use ratatui::layout::Rect;
use unicode_segmentation::UnicodeSegmentation;

/// The cells a Nerd Font glyph takes: itself and a blank one after it.
pub const GLYPH_SLOT: u16 = 2;
/// The separator between the workspace and the views (the chip and the views pad it).
pub const SEP: &str = " │ ";
/// The cells of the workspace's name kept before the views lose their names.
const NAME_KEPT: usize = 8;

/// The workspace chip: the name of the workspace shown, then, after `▾`, each other workspace
/// that wants attention by its letter and its mark (` · B @9`): never a count beside the name
/// shown, which is not its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chip {
    pub name: String,
    /// The other workspaces with something unread: (letter, mark).
    pub others: Vec<(String, String)>,
}

/// A view on the bar: its glyph (icons on), its name, its short name, its count (`@3`, `●2`,
/// `●`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub glyph: Option<&'static str>,
    pub label: String,
    pub short: String,
    pub badge: Option<String>,
}

/// A part of the bar as drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The workspace's color band.
    Band,
    Name,
    /// Another workspace's letter, after `▾`.
    Other,
    /// That workspace's mark (`@9`, `●`).
    Mark,
    /// `▾`.
    Caret,
    Sep,
    /// A Nerd Font glyph: one cell, then a blank one ([`GLYPH_SLOT`]).
    Glyph,
    Label,
    Badge,
    Blank,
}

/// A stretch of the bar: whose (0 the workspace chip, `1 + i` the view `i`; `None` the
/// separator), which part, from column `x`, `width` cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub item: Option<usize>,
    pub part: Part,
    pub x: u16,
    pub text: String,
    pub width: u16,
}

/// How much the views say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    /// Glyphs, names and counts.
    Full,
    /// Names and counts, no glyphs (words read better than glyphs alone).
    Words,
    /// Glyphs (or short names) and counts.
    Short,
    /// Glyphs (or short names) and marks without numbers.
    Compact,
}

/// The bar laid out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bar {
    pub area: Rect,
    pub pieces: Vec<Piece>,
}

impl Bar {
    /// The item at column `x` (0 the workspace chip, `1 + i` view `i`).
    pub fn hit(&self, x: u16) -> Option<usize> {
        self.pieces.iter().find(|p| x >= p.x && x < p.x + p.width).and_then(|p| p.item)
    }

    /// The columns item `item` covers, `[from, to)`.
    pub fn span(&self, item: usize) -> Option<(u16, u16)> {
        let mine = self.pieces.iter().filter(|p| p.item == Some(item));
        let from = mine.clone().map(|p| p.x).min()?;
        Some((from, mine.map(|p| p.x + p.width).max()?))
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

fn parts(chip: &Chip, name: &str, views: &[Item], level: Level) -> Parts {
    let mut out: Parts = Vec::new();
    let mut push = |item: Option<usize>, part: Part, text: String| {
        let w = if part == Part::Glyph { GLYPH_SLOT } else { width(&text) as u16 };
        out.push((item, part, text, w));
    };
    push(Some(0), Part::Blank, " ".into());
    push(Some(0), Part::Band, "▌".into());
    push(Some(0), Part::Name, name.to_string());
    push(Some(0), Part::Caret, " ▾".into());
    for (letter, m) in &chip.others {
        let m = if level == Level::Compact { compact(m) } else { m.clone() };
        push(Some(0), Part::Blank, " · ".into());
        push(Some(0), Part::Other, letter.clone());
        push(Some(0), Part::Mark, format!(" {m}"));
    }
    push(Some(0), Part::Blank, " ".into());
    push(None, Part::Sep, SEP.trim().into());
    for (i, v) in views.iter().enumerate() {
        let item = Some(i + 1);
        push(item, Part::Blank, " ".into());
        let label = match (level, v.glyph) {
            (Level::Full | Level::Words, _) => Some(v.label.clone()),
            (_, Some(_)) => None,
            (_, None) => Some(v.short.clone()),
        };
        if let Some(g) = v.glyph.filter(|_| level != Level::Words) {
            push(item, Part::Glyph, g.to_string());
            if label.is_some() {
                push(item, Part::Blank, " ".into());
            }
        }
        let glyph_alone = v.glyph.is_some() && level != Level::Words && label.is_none() && v.badge.is_none();
        if let Some(l) = label {
            push(item, Part::Label, l);
        }
        if let Some(b) = &v.badge {
            let b = if level == Level::Compact { compact(b) } else { b.clone() };
            push(item, Part::Badge, format!(" {b}"));
        }
        // A glyph alone ends with its slot's blank: the space between views stays two cells.
        if !glyph_alone {
            push(item, Part::Blank, " ".into());
        }
    }
    out
}

fn total(p: &Parts) -> usize {
    p.iter().map(|(.., w)| usize::from(*w)).sum()
}

/// Lay out the bar in `area` (one row).
pub fn layout(area: Rect, chip: &Chip, views: &[Item]) -> Bar {
    let room = usize::from(area.width);
    let full = width(&chip.name);
    // The name's width that makes `level` fit, if cutting it (down to `min` cells) is enough.
    let fit = |level: Level, min: usize| {
        let over = total(&parts(chip, &chip.name, views, level)).saturating_sub(room);
        let want = full.saturating_sub(over);
        (want >= min.min(full)).then_some(want)
    };
    let (level, name_w) =
        [(Level::Full, NAME_KEPT), (Level::Words, NAME_KEPT), (Level::Short, NAME_KEPT), (Level::Compact, NAME_KEPT)]
            .into_iter()
            .chain([(Level::Compact, 1)])
            .find_map(|(level, min)| fit(level, min).map(|w| (level, w)))
            .unwrap_or((Level::Compact, 1));
    let name = if name_w >= full { chip.name.clone() } else { cut_name(&chip.name, name_w) };
    let mut bar = Bar { area, pieces: Vec::new() };
    let mut x = area.x;
    let end = area.x + area.width;
    for (item, part, text, w) in parts(chip, &name, views, level) {
        if x >= end {
            break;
        }
        let left = end - x;
        // A glyph slot that does not fit whole is left out; text is cut.
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
        // Right after a glyph's slot only a blank may come (a glyph drawn wide covers no text).
        if bar.pieces.last().is_some_and(|p| p.part == Part::Glyph) && !text.starts_with(' ') {
            break;
        }
        bar.pieces.push(Piece { item, part, x, text, width: w });
        x += w;
    }
    bar
}

#[cfg(test)]
mod tests;
