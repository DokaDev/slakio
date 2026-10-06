//! The geometry of the main screen, shared by drawing ([`crate::ui`]) and by the mouse and
//! scrolling in the app state, so a click lands on exactly the row that was drawn there.
//!
//! ```text
//!  ▌A company ▾ │ Home  DMs ●2  Activity @3  Files  Later        the top bar (crate::navbar)
//! ╭ list ──────╮╭ main pane ─────────╮╭ thread ──────╮
//! │            ││                    ││              │
//! │            │├─ Message #backend ─┤├─ Reply ──────┤
//! │            ││ › Press i to write ││ ›            │
//! ╰────────────╯╰────────────────────╯╰──────────────╯
//!  status line
//! ```
//!
//! With two tabs or more, the work area's first row is the tab bar ([`crate::tabbar`]).
//!
//! On a narrow screen an open thread panel takes the list panel's room (the list is left out
//! of the layout only, and comes back when the thread closes or the list is focused);
//! narrower still, the pane with the keyboard takes the whole work area.
//!
//! [`frame`] lays out a whole frame once — the regions, then each open pane with its messages
//! and composer — and drawing and the mouse both read that one [`FrameLayout`].

use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use slakio_core::layout::{Area, Node, PaneId, Share};

/// Below this the screen shows only how much room it needs.
pub const MIN_WIDTH: u16 = 50;
pub const MIN_HEIGHT: u16 = 10;
/// A work area narrower than this gives the list panel's room to an open thread panel.
pub const WORK_WIDE: u16 = 88;
/// The main pane keeps this much beside the thread panel, or the focused one takes it all.
pub const MAIN_MIN: u16 = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Areas {
    /// The top bar: the workspace and the views, across the whole width.
    pub nav: Rect,
    pub list: Option<Rect>,
    /// The tab bar, the work area's first row, while there are two tabs or more.
    pub tabs: Option<Rect>,
    pub work: Rect,
    pub status: Rect,
}

pub fn too_small(size: Rect) -> bool {
    size.width < MIN_WIDTH || size.height < MIN_HEIGHT
}

/// What the layout depends on besides the size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shape {
    /// The user hid the list panel.
    pub list_hidden: bool,
    /// The thread panel is open.
    pub thread: bool,
    /// The list panel has the focus (it is never left out then).
    pub list_focused: bool,
    /// There are two tabs or more: the tab bar shows.
    pub tabs: bool,
}

/// The width of the list panel on a screen `width` wide: a fifth or so, 26 to 36 cells.
pub fn list_width(width: u16) -> u16 {
    (width * 22 / 100).clamp(26, 36)
}

/// The areas for a screen of `size`.
pub fn areas(size: Rect, s: Shape) -> Areas {
    let [nav, body, status] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(size);
    let mut list_w = if s.list_hidden { 0 } else { list_width(body.width) };
    let work_w = body.width.saturating_sub(list_w);
    let squeezed = s.thread && work_w < WORK_WIDE && !s.list_focused;
    if squeezed {
        list_w = 0;
    }
    let [list, work] = Layout::horizontal([Constraint::Length(list_w), Constraint::Min(0)]).areas(body);
    let (tabs, work) = match s.tabs && work.height > 1 {
        true => (Some(Rect { height: 1, ..work }), Rect { y: work.y + 1, height: work.height - 1, ..work }),
        false => (None, work),
    };
    Areas { nav, list: (list_w > 0).then_some(list), tabs, work, status }
}

/// The most lines a composer shows before it scrolls.
pub const COMPOSER_MAX_LINES: u16 = 5;

/// How the auto thread panel shares the work area with the conversation: two fifths, 34 to 60
/// cells, while the conversation keeps [`MAIN_MIN`]; where both do not fit, the pane with the
/// keyboard takes the whole area.
pub const THREAD_SHARE: Share = Share { num: 2, den: 5, min: 34, max: 60, keep: MAIN_MIN };

/// The panes of `tree` shown in `work`, and where (`focused` stays where two do not fit).
pub fn work_panes(work: Rect, tree: &Node, focused: Option<PaneId>) -> Vec<(PaneId, Rect)> {
    let area = Area { x: work.x, y: work.y, width: work.width, height: work.height };
    tree.solve(area, focused).into_iter().map(|(id, a)| (id, Rect::new(a.x, a.y, a.width, a.height))).collect()
}

/// Inside a bordered area.
pub fn inner(r: Rect) -> Rect {
    r.inner(Margin { horizontal: 1, vertical: 1 })
}

/// The parts of a pane: its messages, the divider row of its composer (joined to the border) and
/// the composer's lines (` › ` and the text).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneParts {
    pub messages: Rect,
    /// `None` when the pane is too low for a composer.
    pub divider: Option<u16>,
    pub input: Rect,
}

/// The width the composer's text wraps at in a pane `area`: inside the border, one cell of
/// padding each side and the prompt.
pub fn composer_width(area: Rect) -> usize {
    usize::from(inner(area).width.saturating_sub(4)).max(2)
}

/// The parts of pane `area` whose composer shows `lines` lines (at most
/// [`COMPOSER_MAX_LINES`]).
pub fn pane_parts(area: Rect, lines: usize) -> PaneParts {
    let inner = inner(area);
    let lines = (lines as u16).clamp(1, COMPOSER_MAX_LINES);
    if inner.height < lines + 3 {
        return PaneParts { messages: inner, divider: None, input: Rect { height: 0, ..inner } };
    }
    let input = Rect { y: inner.bottom() - lines, height: lines, ..inner };
    let divider = input.y - 1;
    let messages = Rect { height: divider - inner.y, ..inner };
    PaneParts { messages, divider: Some(divider), input }
}

/// A pane as laid out: which, its area and its parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneLayout {
    pub id: PaneId,
    pub rect: Rect,
    pub parts: PaneParts,
}

/// A whole frame: the regions and the panes shown in the work area.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameLayout {
    pub areas: Areas,
    /// The panes shown, in reading order; a pane left out has none.
    pub panes: Vec<PaneLayout>,
}

impl FrameLayout {
    /// Pane `id`, when it is shown.
    pub fn pane(&self, id: PaneId) -> Option<&PaneLayout> {
        self.panes.iter().find(|p| p.id == id)
    }

    /// The pane under `at`.
    pub fn pane_at(&self, at: Position) -> Option<&PaneLayout> {
        self.panes.iter().find(|p| p.rect.contains(at))
    }
}

/// The frame for a screen of `size` whose work area holds `tree` (none: no pane), `focused`
/// the pane with the keyboard. `composer_lines(id, width)` is how many lines the composer of
/// pane `id` takes when it wraps at `width`.
pub fn frame(
    size: Rect,
    s: Shape,
    tree: Option<&Node>,
    focused: Option<PaneId>,
    mut composer_lines: impl FnMut(PaneId, usize) -> usize,
) -> FrameLayout {
    let areas = areas(size, s);
    let shown = tree.map(|t| work_panes(areas.work, t, focused)).unwrap_or_default();
    let panes = shown
        .into_iter()
        .map(|(id, rect)| PaneLayout { id, rect, parts: pane_parts(rect, composer_lines(id, composer_width(rect))) })
        .collect();
    FrameLayout { areas, panes }
}

/// The width of the workspace switcher.
pub const SWITCHER_WIDTH: u16 = 32;

/// Where the workspace switcher is drawn on a screen `size`: under the top bar, from column `x`
/// (the chip's), one row per workspace of `n` (the screen keeps it inside).
pub fn switcher(size: Rect, x: u16, n: usize) -> Rect {
    let w = SWITCHER_WIDTH.min(size.width);
    let h = (n as u16 + 2).min(size.height.saturating_sub(2));
    Rect::new(x.min(size.width - w), size.y + 1, w, h)
}

/// The most entries the command palette lists at once (the list scrolls).
pub const PALETTE_ROWS: usize = 12;

/// Where the command palette is drawn: a box near the top, centered, its first line the input,
/// a rule, then the entries and, under them, `extra` lines (why `Enter` did nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteBox {
    pub rect: Rect,
    /// The input line (inside the border).
    pub input: Rect,
    /// The entries shown, one per line.
    pub list: Rect,
    /// The first entry shown (the selected one is kept in view).
    pub first: usize,
    /// Below the entries.
    pub extra: Rect,
}

/// The palette's width on a screen `width` wide: 60%, 60 to 100 cells, never wider than the
/// screen less a margin.
pub fn palette_width(width: u16) -> u16 {
    (width * 6 / 10).clamp(60, 100).min(width.saturating_sub(4)).max(8)
}

/// The palette for a screen of `size` listing `entries`, `selected` among them, with `extra`
/// lines under them; `None` on a screen too small for it.
pub fn palette(size: Rect, entries: usize, selected: usize, extra: u16) -> Option<PaletteBox> {
    if size.height < 6 || size.width < 12 {
        return None;
    }
    let w = palette_width(size.width);
    let x = size.x + (size.width - w) / 2;
    let top = size.y + (size.height / 6).max(1);
    // Borders, input and rule take four lines; the status line stays clear.
    let room = usize::from((size.y + size.height).saturating_sub(top + 4 + extra + 1));
    let n = entries.clamp(1, PALETTE_ROWS.min(room.max(1)));
    let rect = Rect::new(x, top, w, 4 + n as u16 + extra);
    let inner = inner(rect);
    let list = Rect { y: inner.y + 2, height: n as u16, ..inner };
    Some(PaletteBox {
        rect,
        input: Rect { height: 1, ..inner },
        list,
        first: selected.saturating_sub(n - 1),
        extra: Rect { y: list.bottom(), height: extra, ..inner },
    })
}

#[cfg(test)]
mod tests;
