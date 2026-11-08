//! The mouse over the main screen: the list panel's chip and views (click), the list
//! (click, wheel), the tab
//! bar ([`super::tabs`]) and the panes (click, double click, the reply link, the composer,
//! wheel), the borders between panes (drag). Popups take the mouse first ([`super::overlay`]). Where things are comes from the
//! frame's one layout ([`crate::screen`]), as drawn.

use super::model::Row;
use super::{App, DOUBLE_CLICK, Focus, Guide, Overlay, WHEEL_ROWS};
use crate::action::ShellAction;
use crate::screen;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;
use std::time::{Duration, Instant};

/// How long after a pane's links moved a click on one only selects its message.
pub const LINK_SETTLE: Duration = Duration::from_millis(400);

impl App {
    /// The mouse: a click on a view shows it, on the workspace chip opens the switcher, on a list row
    /// opens it, on the tab bar goes there ([`super::tabs`]), on a message selects it (twice: its
    /// thread), on a reply link opens the thread, on a composer writes in it; the wheel scrolls
    /// what is under it. `true` when the screen changed.
    pub(super) fn mouse(&mut self, m: MouseEvent, now: Instant) -> bool {
        match self.overlay() {
            Some(Overlay::Dialog(_)) => return false,
            Some(Overlay::Palette) => return self.palette_mouse(m, now),
            Some(Overlay::Search) => return self.search_mouse(m, now),
            Some(Overlay::Switcher) => return self.switcher_mouse(m),
            _ => {}
        }
        let picking = self.picker.is_some();
        if let Some(redraw) = self.pick_mouse(m.kind, Position { x: m.column, y: m.row }, now) {
            return redraw;
        }
        // Letters stopped by a press or the wheel are drawn away, whatever else it did.
        let stopped = picking && self.picker.is_none();
        self.mouse_screen(m, now) || stopped
    }

    /// The mouse over the main screen, no popup taking it.
    fn mouse_screen(&mut self, m: MouseEvent, now: Instant) -> bool {
        // The sign-in screen over the app (`:login`) takes no click.
        if self.backend.is_none() || self.sign_in.state.is_some() || screen::too_small(self.size) {
            return false;
        }
        let at = Position { x: m.column, y: m.row };
        if let Some(h) = self.help.as_mut() {
            let by = match m.kind {
                MouseEventKind::ScrollDown => 1,
                MouseEventKind::ScrollUp => -1,
                _ => return false,
            };
            let n = h.rows(&self.keymap, &self.i18n).len();
            h.step(by, n);
            return true;
        }
        let a = self.areas();
        let on_tabs = a.tabs.is_some_and(|r| r.contains(at));
        match m.kind {
            // A tab pressed follows the mouse along the bar until the button is let go.
            MouseEventKind::Drag(MouseButton::Left) if self.tab_drag.is_some() => self.tab_bar_mouse(m, false),
            // A border between panes pressed follows the mouse the same way.
            MouseEventKind::Drag(MouseButton::Left) if self.seam_drag.is_some() => self.seam_move(at, now),
            MouseEventKind::Up(MouseButton::Left) => self.tab_drag.take().is_some() | self.seam_release(),
            MouseEventKind::Down(MouseButton::Left) if a.work.contains(at) && self.seam_press(at) => {
                self.keys.clear();
                self.guide = Guide::default();
                true
            }
            MouseEventKind::Down(MouseButton::Middle) if on_tabs => self.tab_bar_mouse(m, false),
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let by = if m.kind == MouseEventKind::ScrollDown { 1 } else { -1 };
                if a.list.is_some_and(|l| l.contains(at)) {
                    let height = self.list_height();
                    self.shell.scroll_by(&self.model, by * WHEEL_ROWS, height);
                    return true;
                }
                let id = self.frame().pane_at(at).map(|p| p.id);
                self.work.step_in(id, by);
                true
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.keys.clear();
                self.guide = Guide::default();
                let double = self.last_click.is_some_and(|(t, p)| p == at && now.duration_since(t) <= DOUBLE_CLICK);
                self.last_click = Some((now, at));
                if let Some(list) = a.list.filter(|l| l.contains(at)) {
                    let rows = screen::list_parts(list, self.nav_rows()).rows;
                    // Below the rows shown (Activity's end) is not a row.
                    let row_at = usize::from(at.y.saturating_sub(rows.y));
                    if !self.nav_click(at, now) && rows.contains(at) && row_at < self.list_height() {
                        let row = self.shell.list_top + row_at;
                        self.click_list(row, at.x - rows.x, now);
                    }
                } else if on_tabs {
                    self.tab_bar_mouse(m, double);
                } else if a.work.contains(at) {
                    self.click_work(at, double, now);
                }
                true
            }
            _ => false,
        }
    }

    /// A click on row `row` of the list, `x` cells from its left edge: a conversation or an
    /// item opens (a to-do's second line is its item's), a header folds, the filter row opens
    /// the values of the part clicked.
    fn click_list(&mut self, row: usize, x: u16, now: Instant) {
        let rows = self.shell.rows(&self.model);
        let row = if matches!(rows.get(row), Some(Row::ItemMeta(_) | Row::NoticeMeta(_))) { row - 1 } else { row };
        if !rows.get(row).is_some_and(|r| r.is_selectable()) {
            return;
        }
        self.set_focus(Focus::List);
        self.shell.list_cursor = row;
        match rows[row] {
            Row::Filter if x >= self.filter_row().sort_x => self.todo(ShellAction::TodoSort, now),
            Row::Filter => self.todo(ShellAction::TodoFilter, now),
            Row::Item(_) => self.open_item(true),
            Row::Notice(_) => self.open_notice(true, now),
            _ => {
                if let Some(t) = self.shell.open_row(&self.model) {
                    self.open(t, true);
                }
            }
        }
    }

    /// A click in the work area at `at` (`double`: the second of a double click); on a link of a
    /// message, it is selected and the link opened once (not again by a double click, and not
    /// within [`LINK_SETTLE`] of the pane's links moving: then it only selects).
    fn click_work(&mut self, at: Position, double: bool, now: Instant) {
        let Some(&layout) = self.frame().pane_at(at) else { return };
        self.set_focus(Focus::on(layout.id));
        let Some(pane) = self.work.focused() else { return };
        let parts = layout.parts;
        let hit = pane.hit(at.y).filter(|_| parts.messages.contains(at));
        let link = pane.links.borrow().iter().find(|l| l.y == at.y && (l.x0..l.x1).contains(&at.x)).cloned();
        // Just after the links moved, what is under the pointer may not be what was aimed at.
        let settled = pane.links_moved.get().is_none_or(|t| now.duration_since(t) >= LINK_SETTLE);
        self.work.set_insert(parts.input.contains(at));
        let Some(hit) = hit else { return };
        self.work.with_pane(|p, tl| {
            p.select_index(hit.message, tl);
            p.visual = None;
        });
        match link {
            // The second press of a double click opened it already.
            Some(l) if settled && !double => self.open_clicked(layout.id, &l, now),
            Some(_) => {}
            None if hit.link || double => self.open_thread(),
            None => {}
        }
    }
}
