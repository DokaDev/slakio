//! The geometry of the main screen, shared by drawing ([`crate::ui`]) and by the mouse and
//! scrolling in the app state, so a click lands on exactly the row that was drawn there.
//!
//! ```text
//! ╭──╮╭ list ──────╮╭ main pane ─────────╮╭ thread ──────╮
//! │▌A││            ││                    ││              │
//! │  ││            │├─ Message #backend ─┤├─ Reply ──────┤
//! │  ││            ││ › Press i to write ││ ›            │
//! ╰──╯╰────────────╯╰────────────────────╯╰──────────────╯
//!  status line
//! ```
//!
//! The focused (or hovered) rail widens to show labels, either over the list panel or pushing
//! it aside. On a narrow screen an open thread panel takes the list panel's room (the list is
//! left out of the layout only, and comes back when the thread closes or the list is focused);
//! narrower still, the pane with the keyboard takes the whole work area.
//!
//! [`frame`] lays out a whole frame once — the regions, then each open pane with its messages
//! and composer — and drawing and the mouse both read that one [`FrameLayout`].

use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};

/// The collapsed rail: a border, the workspace stripe and one letter or icon, and a dot.
pub const RAIL_WIDTH: u16 = 4;
/// The expanded rail.
pub const RAIL_EXPANDED_WIDTH: u16 = 21;
/// Below this the screen shows only how much room it needs.
pub const MIN_WIDTH: u16 = 50;
pub const MIN_HEIGHT: u16 = 10;
/// A work area narrower than this gives the list panel's room to an open thread panel.
pub const WORK_WIDE: u16 = 88;
/// The main pane keeps this much beside the thread panel, or the focused one takes it all.
pub const MAIN_MIN: u16 = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Areas {
    pub rail: Rect,
    pub list: Option<Rect>,
    pub work: Rect,
    pub status: Rect,
}

pub fn too_small(size: Rect) -> bool {
    size.width < MIN_WIDTH || size.height < MIN_HEIGHT
}

/// What the layout depends on besides the size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shape {
    pub rail_expanded: bool,
    /// An expanded rail pushes the list aside instead of covering it.
    pub push: bool,
    /// The user hid the list panel.
    pub list_hidden: bool,
    /// The thread panel is open.
    pub thread: bool,
    /// The list panel has the focus (it is never left out then).
    pub list_focused: bool,
    /// A conversation is open in the work area.
    pub main: bool,
    /// The thread panel has the keyboard (on a screen too narrow for both panes, it is shown).
    pub thread_focused: bool,
}

/// The width of the list panel on a screen `width` wide: a fifth or so, 26 to 36 cells.
pub fn list_width(width: u16) -> u16 {
    (width * 22 / 100).clamp(26, 36)
}

/// The areas for a screen of `size`.
pub fn areas(size: Rect, s: Shape) -> Areas {
    let [body, status] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(size);
    let rail_w = if s.rail_expanded && s.push { RAIL_EXPANDED_WIDTH } else { RAIL_WIDTH };
    let mut list_w = if s.list_hidden { 0 } else { list_width(body.width) };
    let work_w = body.width.saturating_sub(rail_w + list_w);
    let squeezed = s.thread && work_w < WORK_WIDE && !s.list_focused;
    if squeezed {
        list_w = 0;
    }
    let [rail, list, work] =
        Layout::horizontal([Constraint::Length(rail_w), Constraint::Length(list_w), Constraint::Min(0)]).areas(body);
    let rail =
        if s.rail_expanded && !s.push { Rect { width: RAIL_EXPANDED_WIDTH.min(body.width), ..rail } } else { rail };
    Areas { rail, list: (list_w > 0).then_some(list), work, status }
}

/// The most lines a composer shows before it scrolls.
pub const COMPOSER_MAX_LINES: u16 = 5;

/// The work area split into the main pane and, when open, the thread panel on its right (two
/// fifths, 34 to 60 cells, while the main pane keeps [`MAIN_MIN`]). Where both do not fit, the
/// pane with the keyboard (`thread_focused`) takes the whole area.
pub fn work_split(work: Rect, thread: bool, thread_focused: bool) -> (Option<Rect>, Option<Rect>) {
    if !thread {
        return (Some(work), None);
    }
    let w = (work.width * 2 / 5).clamp(34, 60);
    if work.width < w + MAIN_MIN {
        return if thread_focused { (None, Some(work)) } else { (Some(work), None) };
    }
    let [main, side] = Layout::horizontal([Constraint::Min(0), Constraint::Length(w)]).areas(work);
    (Some(main), Some(side))
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

/// A pane's place in the work area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// The conversation open in the work area.
    Main,
    /// The auto thread panel beside it.
    Thread,
}

/// A pane as laid out: its place, its area and its parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneLayout {
    pub slot: Slot,
    pub rect: Rect,
    pub parts: PaneParts,
}

/// A whole frame: the regions and the panes shown in the work area.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameLayout {
    pub areas: Areas,
    /// The panes shown, the main pane first; a pane left out has none.
    pub panes: Vec<PaneLayout>,
}

impl FrameLayout {
    /// The pane in `slot`, when it is shown.
    pub fn pane(&self, slot: Slot) -> Option<&PaneLayout> {
        self.panes.iter().find(|p| p.slot == slot)
    }

    /// The pane under `at`.
    pub fn pane_at(&self, at: Position) -> Option<&PaneLayout> {
        self.panes.iter().find(|p| p.rect.contains(at))
    }
}

/// The frame for a screen of `size`. `composer_lines(slot, width)` is how many lines the
/// composer of the pane in `slot` takes when it wraps at `width`.
pub fn frame(size: Rect, s: Shape, composer_lines: impl Fn(Slot, usize) -> usize) -> FrameLayout {
    let areas = areas(size, s);
    let mut panes = Vec::new();
    if s.main {
        let (main, thread) = work_split(areas.work, s.thread, s.thread_focused);
        for (slot, rect) in [(Slot::Main, main), (Slot::Thread, thread)] {
            if let Some(rect) = rect {
                let parts = pane_parts(rect, composer_lines(slot, composer_width(rect)));
                panes.push(PaneLayout { slot, rect, parts });
            }
        }
    }
    FrameLayout { areas, panes }
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

/// The rail item (index into [`crate::app::shell::rail_items`]) on screen row `y`, given
/// `workspaces` workspaces: workspaces first, a separator line, then the views.
pub fn rail_item_at(rail: Rect, workspaces: usize, items: usize, y: u16) -> Option<usize> {
    let inner = inner(rail);
    if y < inner.y || y >= inner.bottom() {
        return None;
    }
    let row = usize::from(y - inner.y);
    let item = match row {
        r if r < workspaces => r,
        r if r == workspaces => return None,
        r => r - 1,
    };
    (item < items).then_some(item)
}

#[cfg(test)]
mod tests;
