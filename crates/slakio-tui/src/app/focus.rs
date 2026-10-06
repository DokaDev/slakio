//! Where the keyboard is: one [`Focus`] — the rail, the list panel or a pane — read by the key
//! map, drawing and the mouse alike, and changed in one place ([`App::set_focus`]). The shell
//! keeps which region it is, the work area which of its panes is the active one.

use super::App;
use super::query::{Focus, PaneHandle};
use super::shell::Region;
use super::work::Side;
use crate::screen::Slot;

impl Focus {
    /// The focus on the pane laid out in `slot`.
    pub(crate) fn on(slot: Slot) -> Self {
        Self::Pane(PaneHandle::of(slot))
    }

    /// A pane has the keyboard.
    pub fn is_pane(self) -> bool {
        matches!(self, Self::Pane(_))
    }
}

impl App {
    /// Where the keyboard is, under any popup.
    pub fn focus(&self) -> Focus {
        match self.shell.focus {
            Region::Rail => Focus::Rail,
            Region::List => Focus::List,
            Region::Work => Focus::on(self.work.side.slot()),
        }
    }

    /// Give the keyboard to `focus`. Moving to another pane leaves Insert mode.
    pub(crate) fn set_focus(&mut self, focus: Focus) {
        match focus {
            Focus::Rail => self.shell.focus = Region::Rail,
            Focus::List => self.shell.focus = Region::List,
            Focus::Pane(h) => {
                self.shell.focus = Region::Work;
                let side = Side::of(h.slot());
                if side != self.work.side {
                    self.work.set_insert(false);
                    self.work.turn(side);
                }
            }
        }
    }

    /// The keyboard goes to the work area's active pane.
    pub(crate) fn focus_work(&mut self) {
        self.set_focus(Focus::on(self.work.side.slot()));
    }
}
