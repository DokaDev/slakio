//! Where the search popup is drawn, which the drawing and the mouse both read: a box near the
//! top, as wide as the palette, as tall as the screen allows (up to [`HITS_MAX`] results, two
//! lines each), the result under the cursor kept in view. The wheel moves the cursor, a click on
//! a result opens it, a click outside closes the popup.

use super::super::App;
use crate::action::SearchAction;
use crate::screen;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use std::time::Instant;

/// The most results shown at once.
pub const HITS_MAX: u16 = 10;

/// Lines a result takes.
pub const HIT_LINES: u16 = 2;

/// The search popup's place on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchBox {
    pub rect: Rect,
    /// The words' line (inside the border).
    pub input: Rect,
    /// Where the results go ([`HIT_LINES`] lines each), or what is said instead.
    pub list: Rect,
    /// The first result shown.
    pub first: usize,
}

impl SearchBox {
    /// Results shown at once.
    pub fn shown(&self) -> usize {
        usize::from(self.list.height / HIT_LINES)
    }
}

/// The popup on a screen of `size` with result `selected` under the cursor; `None` on a screen
/// too small for it.
pub fn search_box(size: Rect, selected: usize) -> Option<SearchBox> {
    if size.height < 10 || size.width < 20 {
        return None;
    }
    let w = screen::palette_width(size.width);
    let x = size.x + (size.width - w) / 2;
    let top = size.y + (size.height / 8).max(1);
    // Borders, the words and a rule take four lines; the status line stays clear.
    let room = (size.y + size.height).saturating_sub(top + 4 + 1);
    let list_h = (room / HIT_LINES * HIT_LINES).clamp(HIT_LINES, HITS_MAX * HIT_LINES);
    let rect = Rect::new(x, top, w, 4 + list_h);
    let inner = Rect { x: rect.x + 1, y: rect.y + 1, width: rect.width.saturating_sub(2), height: rect.height - 2 };
    let list = Rect { y: inner.y + 2, height: list_h, ..inner };
    let shown = usize::from(list_h / HIT_LINES).max(1);
    Some(SearchBox { rect, input: Rect { height: 1, ..inner }, list, first: selected.saturating_sub(shown - 1) })
}

impl App {
    /// Where the search popup is drawn now (`None` when closed, or the screen is too small).
    pub fn search_box(&self) -> Option<SearchBox> {
        self.search.open.then(|| search_box(self.size, self.search.selected)).flatten()
    }

    /// The mouse over the search popup. `true` when the screen changed.
    pub(in crate::app) fn search_mouse(&mut self, m: MouseEvent, now: Instant) -> bool {
        let Some(b) = self.search_box() else { return false };
        let at = Position { x: m.column, y: m.row };
        let hits = self.search.found.as_ref().map_or(0, |f| f.hits.len());
        match m.kind {
            MouseEventKind::ScrollDown => self.search_action(SearchAction::Next, now),
            MouseEventKind::ScrollUp => self.search_action(SearchAction::Prev, now),
            MouseEventKind::Down(MouseButton::Left) if b.list.contains(at) => {
                let i = b.first + usize::from((at.y - b.list.y) / HIT_LINES);
                if i >= hits || self.search_showing() != super::Showing::Results {
                    return false;
                }
                self.search.selected = i;
                self.open_hit(now);
            }
            MouseEventKind::Down(MouseButton::Left) if !b.rect.contains(at) => self.close_search(),
            _ => return false,
        }
        true
    }
}
