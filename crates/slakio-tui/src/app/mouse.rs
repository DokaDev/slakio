//! The mouse over the main screen: the list panel's chip and views (click), the list
//! (click, wheel), the tab
//! bar ([`super::tabs`]) and the panes (click, double click, the reply link, the composer,
//! wheel). Popups take the mouse first ([`super::overlay`]). Where things are comes from the
//! frame's one layout ([`crate::screen`]), as drawn.

use super::{App, DOUBLE_CLICK, Focus, Guide, Overlay, WHEEL_ROWS};
use crate::screen;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;
use std::time::Instant;

impl App {
    /// The mouse: a click on a view shows it, on the workspace chip opens the switcher, on a list row
    /// opens it, on the tab bar goes there ([`super::tabs`]), on a message selects it (twice: its
    /// thread), on a reply link opens the thread, on a composer writes in it; the wheel scrolls
    /// what is under it. `true` when the screen changed.
    pub(super) fn mouse(&mut self, m: MouseEvent, now: Instant) -> bool {
        match self.overlay() {
            Some(Overlay::Dialog(_)) => return false,
            Some(Overlay::Palette) => return self.palette_mouse(m, now),
            Some(Overlay::Switcher) => return self.switcher_mouse(m),
            _ => {}
        }
        if self.backend.is_none() || screen::too_small(self.size) {
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
            MouseEventKind::Up(MouseButton::Left) => self.tab_drag.take().is_some(),
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
                    if !self.nav_click(at, now) && rows.contains(at) {
                        let row = self.shell.list_top + usize::from(at.y - rows.y);
                        if self.shell.rows(&self.model).get(row).is_some_and(|r| r.is_selectable()) {
                            self.set_focus(Focus::List);
                            self.shell.list_cursor = row;
                            if let Some(t) = self.shell.open_row(&self.model) {
                                self.open(t, true);
                            }
                        }
                    }
                } else if on_tabs {
                    self.tab_bar_mouse(m, double);
                } else if a.work.contains(at) {
                    self.click_work(at, double);
                }
                true
            }
            _ => false,
        }
    }

    /// A click in the work area at `at` (`double`: the second of a double click).
    fn click_work(&mut self, at: Position, double: bool) {
        let Some(&layout) = self.frame().pane_at(at) else { return };
        self.set_focus(Focus::on(layout.id));
        let Some(pane) = self.work.focused() else { return };
        let parts = layout.parts;
        let hit = pane.hit(at.y).filter(|_| parts.messages.contains(at));
        self.work.set_insert(parts.input.contains(at));
        let Some(hit) = hit else { return };
        self.work.with_pane(|p, tl| {
            p.select_index(hit.message, tl);
            p.visual = None;
        });
        if hit.link || double {
            self.open_thread();
        }
    }
}
