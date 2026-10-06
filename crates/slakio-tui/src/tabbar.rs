//! The tab bar's geometry, shared by drawing ([`crate::ui`]) and the mouse, so a click lands
//! on exactly what was drawn there. Each tab is ` <n> <title> [●<unread>] × `; the tab shown is
//! drawn apart by its style only. When the tabs do not fit, the titles are shortened in steps
//! ([`TITLE_STEPS`]), then the bar shows a window of tabs around the one shown, with `‹` / `›`
//! at the ends for the tabs left out on that side (a click shows the nearest of them).
//!
//! ```text
//!  1 #backend ×  2 ⤷ Deploy rollback ●3 ×  3 @Minsu Kim ×
//! ‹ 3 @Minsu Kim ×  4 #incidents ●1 ×  5 #general ×        ›
//! ```

use crate::text::{clip, width};
use ratatui::layout::Rect;

/// The close button of a tab.
pub const CLOSE: &str = "×";
/// Tabs left out on the left, on the right.
pub const MORE_LEFT: &str = "‹";
pub const MORE_RIGHT: &str = "›";
/// The longest a title is drawn when the tabs do not fit, tried in turn.
pub const TITLE_STEPS: [usize; 4] = [24, 16, 10, 6];

/// What a tab says: its number, its title and its unread count, if any (`●3`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub number: String,
    pub title: String,
    pub badge: Option<String>,
}

/// A part of a tab as drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// ` 1 `
    Number,
    Title,
    /// ` ●3`
    Badge,
    /// The spaces around the close button.
    Blank,
    Close,
}

/// A stretch of text of the bar: whose tab, which part, from column `x`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub tab: usize,
    pub part: Part,
    pub x: u16,
    pub text: String,
}

/// Where a click on the bar leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The tab at this index.
    Tab(usize),
    /// The close button of the tab at this index.
    Close(usize),
    /// A `‹` / `›` mark: the nearest tab left out on its side.
    More(usize),
}

/// The bar laid out: the pieces drawn, left to right, and the marks for tabs left out (their
/// column and the tab a click shows).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bar {
    pub area: Rect,
    pub pieces: Vec<Piece>,
    pub left: Option<(u16, usize)>,
    pub right: Option<(u16, usize)>,
}

impl Bar {
    /// What column `x` of the bar leads to.
    pub fn hit(&self, x: u16) -> Option<Hit> {
        match (self.left, self.right) {
            (Some((at, i)), _) if at == x => return Some(Hit::More(i)),
            (_, Some((at, i))) if at == x => return Some(Hit::More(i)),
            _ => {}
        }
        let p = self.pieces.iter().find(|p| x >= p.x && usize::from(x - p.x) < width(&p.text))?;
        Some(if p.part == Part::Close { Hit::Close(p.tab) } else { Hit::Tab(p.tab) })
    }

    /// The tab whose label covers column `x` (the close button included), for dragging.
    pub fn tab_at(&self, x: u16) -> Option<usize> {
        match self.hit(x)? {
            Hit::Tab(i) | Hit::Close(i) => Some(i),
            Hit::More(_) => None,
        }
    }
}

/// The parts of tab label `l`, its title at most `max` columns.
fn parts(l: &Label, max: usize) -> Vec<(Part, String)> {
    let mut out = vec![(Part::Number, format!(" {} ", l.number)), (Part::Title, clip(&l.title, max.max(1)))];
    if let Some(b) = &l.badge {
        out.push((Part::Badge, format!(" {b}")));
    }
    out.extend([(Part::Blank, " ".to_string()), (Part::Close, CLOSE.to_string()), (Part::Blank, " ".to_string())]);
    out
}

fn parts_width(parts: &[(Part, String)]) -> usize {
    parts.iter().map(|(_, t)| width(t)).sum()
}

/// The tabs `first..=last` around `current` that fit in `room` columns.
fn window(widths: &[usize], current: usize, room: usize) -> (usize, usize) {
    let (mut first, mut last, mut used) = (current, current, widths[current]);
    loop {
        let right = last + 1 < widths.len() && used + widths[last + 1] <= room;
        if right {
            last += 1;
            used += widths[last];
        }
        let left = first > 0 && used + widths[first - 1] <= room;
        if left {
            first -= 1;
            used += widths[first];
        }
        if !right && !left {
            return (first, last);
        }
    }
}

/// Lay out `labels` in `area`, `current` the tab shown (kept in view).
pub fn layout(area: Rect, labels: &[Label], current: usize) -> Bar {
    let mut bar = Bar { area, ..Bar::default() };
    let total = usize::from(area.width);
    if labels.is_empty() || total == 0 {
        return bar;
    }
    let current = current.min(labels.len() - 1);
    let make = |max: usize| labels.iter().map(|l| parts(l, max)).collect::<Vec<_>>();
    let mut all = make(usize::MAX);
    for max in TITLE_STEPS {
        if all.iter().map(|p| parts_width(p)).sum::<usize>() <= total {
            break;
        }
        all = make(max);
    }
    let widths: Vec<usize> = all.iter().map(|p| parts_width(p)).collect();
    let (first, last) = if widths.iter().sum::<usize>() <= total {
        (0, labels.len() - 1)
    } else {
        // A column at each end for the marks.
        window(&widths, current, total.saturating_sub(2))
    };
    let mut x = area.x;
    let end = area.x + area.width;
    if first > 0 {
        bar.left = Some((x, first - 1));
        x += 1;
    }
    let stop = if last + 1 < labels.len() { end.saturating_sub(1) } else { end };
    for (i, parts) in all.into_iter().enumerate().take(last + 1).skip(first) {
        for (part, text) in parts {
            if x >= stop {
                break;
            }
            let text = clip(&text, usize::from(stop - x));
            let w = width(&text) as u16;
            bar.pieces.push(Piece { tab: i, part, x, text });
            x += w;
        }
    }
    if last + 1 < labels.len() {
        bar.right = Some((end - 1, last + 1));
    }
    bar
}

#[cfg(test)]
mod tests;
