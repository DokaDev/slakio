//! Where the keyboard is: the rail, the list panel or the work area — one [`Region`] field of
//! the app, changed in one place ([`App::set_focus`]) and read by the key map, drawing and the
//! mouse alike ([`App::focus`]). Which pane of the work area has it is not kept twice: it is the
//! work area's active pane (the one the keyboard goes back to there), so the focus can never
//! name a pane that is closed. The shell and the work area never move the focus: they hand back
//! where it should go.

use super::App;
use super::query::{Focus, PaneHandle};
use super::shell::Region;
use slakio_core::layout::PaneId;

impl Focus {
    /// The focus on pane `id`.
    pub(crate) fn on(id: PaneId) -> Self {
        Self::Pane(PaneHandle::of(id))
    }

    /// A pane has the keyboard.
    pub fn is_pane(self) -> bool {
        matches!(self, Self::Pane(_))
    }
}

impl App {
    /// Where the keyboard is, under any popup: in the work area, its active pane (or the work
    /// area itself, with no pane open).
    pub fn focus(&self) -> Focus {
        match self.region {
            Region::Rail => Focus::Rail,
            Region::List => Focus::List,
            Region::Work => self.work.active().map_or(Focus::Work, Focus::on),
        }
    }

    /// Give the keyboard to `focus`. Moving to another pane leaves Insert mode; a pane that is
    /// not open takes nothing.
    pub(crate) fn set_focus(&mut self, focus: Focus) {
        self.region = match focus {
            Focus::Rail => Region::Rail,
            Focus::List => Region::List,
            Focus::Work => Region::Work,
            Focus::Pane(h) if self.work.pane(h.id()).is_some() => {
                if self.work.active() != Some(h.id()) {
                    self.work.set_insert(false);
                    self.work.activate(h.id());
                }
                Region::Work
            }
            Focus::Pane(_) => {
                debug_assert!(false, "focus on a pane that is not open: {focus:?}");
                return;
            }
        };
    }

    /// The keyboard goes to the work area (its active pane).
    pub(crate) fn focus_work(&mut self) {
        self.set_focus(Focus::Work);
    }

    /// The region the focus is in, as the shell knows them.
    pub(crate) fn region(&self) -> Region {
        self.region
    }

    /// The focus goes to `region` (the work area: its active pane).
    pub(crate) fn focus_region(&mut self, region: Region) {
        match region {
            Region::Rail => self.set_focus(Focus::Rail),
            Region::List => self.set_focus(Focus::List),
            Region::Work => self.focus_work(),
        }
    }

    /// Move the focus to the next (`1`) or previous (`-1`) panel: rail, list, the panes in
    /// reading order (the conversation, the thread panel beside it), round again. A hidden list
    /// or a closed pane is skipped. The rail is expanded only while it has the focus, so passing
    /// it never leaves it open.
    pub(super) fn cycle(&mut self, step: isize) {
        let mut stops = vec![Focus::Rail];
        if !self.shell.list_hidden {
            stops.push(Focus::List);
        }
        stops.extend(self.work.ids().into_iter().map(Focus::on));
        let at = stops.iter().position(|s| *s == self.focus());
        let n = stops.len() as isize;
        let to = match at {
            Some(i) => stops[(i as isize + step).rem_euclid(n) as usize],
            None => stops[0],
        };
        self.work.set_insert(false);
        self.set_focus(to);
    }

    /// The list panel gets the keyboard, its cursor on the conversation `target` names.
    pub(super) fn focus_list(&mut self, target: Option<slakio_core::model::Target>) {
        self.work.set_insert(false);
        self.shell.list_hidden = false;
        self.set_focus(Focus::List);
        if let Some(t) = target {
            let height = self.list_height();
            self.shell.reveal(&self.model, &t, height);
        }
    }

    /// `Esc` in a pane: one step out (see the table at the top).
    pub(super) fn escape(&mut self, main: Option<slakio_core::model::Target>) {
        let Some(p) = self.work.focused_mut() else {
            return self.focus_list(None);
        };
        if p.visual.take().is_some() {
            return;
        }
        if p.selected.take().is_some() {
            p.bottom.set(None);
            return;
        }
        if let Some(owner) = self.work.active().and_then(|a| self.work.owner(a)) {
            return self.set_focus(Focus::on(owner));
        }
        if !self.shell.list_hidden {
            self.focus_list(main);
        }
    }
}
