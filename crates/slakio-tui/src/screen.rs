//! The geometry of the main screen, shared by drawing ([`crate::ui`]) and by the mouse and
//! scrolling in the app state, so a click lands on exactly the row that was drawn there.
//!
//! ```text
//! ╭──╮╭ list ──────╮╭ work area ─────────────╮
//! │▌A││            ││                        │
//! ╰──╯╰────────────╯╰────────────────────────╯
//!  status line
//! ```
//!
//! The focused (or hovered) rail widens to show labels, either over the list panel or pushing
//! it aside.

use ratatui::layout::{Constraint, Layout, Margin, Rect};

/// The collapsed rail: a border, the workspace stripe and one letter or icon, and a dot.
pub const RAIL_WIDTH: u16 = 4;
/// The expanded rail.
pub const RAIL_EXPANDED_WIDTH: u16 = 21;
/// Below this the screen shows only how much room it needs.
pub const MIN_WIDTH: u16 = 50;
pub const MIN_HEIGHT: u16 = 10;

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

/// The areas for a screen of `size`. `push`: an expanded rail pushes the list aside instead of
/// covering it.
pub fn areas(size: Rect, rail_expanded: bool, push: bool, list_hidden: bool) -> Areas {
    let [body, status] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(size);
    let rail_w = if rail_expanded && push { RAIL_EXPANDED_WIDTH } else { RAIL_WIDTH };
    let list_w = if list_hidden { 0 } else { (body.width / 5).clamp(22, 32) };
    let [rail, list, work] =
        Layout::horizontal([Constraint::Length(rail_w), Constraint::Length(list_w), Constraint::Min(0)]).areas(body);
    let rail = if rail_expanded && !push { Rect { width: RAIL_EXPANDED_WIDTH.min(body.width), ..rail } } else { rail };
    Areas { rail, list: (!list_hidden).then_some(list), work, status }
}

/// The most lines a composer shows before it scrolls.
pub const COMPOSER_MAX_LINES: u16 = 5;

/// The work area split into the main pane and, when open, the thread panel on its right (two
/// fifths of the width, at least 30 cells while the main pane keeps 20).
pub fn work_split(work: Rect, thread: bool) -> (Rect, Option<Rect>) {
    if !thread {
        return (work, None);
    }
    let w = (work.width * 2 / 5).max(30).min(work.width.saturating_sub(20));
    let [main, side] = Layout::horizontal([Constraint::Min(0), Constraint::Length(w)]).areas(work);
    (main, Some(side))
}

/// Inside a bordered area.
pub fn inner(r: Rect) -> Rect {
    r.inner(Margin { horizontal: 1, vertical: 1 })
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
