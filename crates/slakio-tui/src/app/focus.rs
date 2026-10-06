//! Where the keyboard is: one [`Focus`] field of the app — the rail, the list panel or a pane —
//! read by the key map, drawing and the mouse alike, and changed in one place
//! ([`App::set_focus`]). The shell and the work area never move it: they hand back where it
//! should go. The work area keeps its active pane (where the keyboard goes back to); a pane
//! with the focus is always the active one.

use super::App;
use super::query::{Focus, PaneHandle};
use super::shell::Region;
use slakio_core::layout::PaneId;

/// The work area with no pane open: what the focus names when it is there.
pub(crate) const NO_PANE: PaneId = PaneId(0);

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
    /// Where the keyboard is, under any popup.
    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// Give the keyboard to `focus`. Moving to another pane leaves Insert mode.
    pub(crate) fn set_focus(&mut self, focus: Focus) {
        if let Focus::Pane(h) = focus
            && self.work.pane(h.id()).is_some()
            && self.work.active() != Some(h.id())
        {
            self.work.set_insert(false);
            self.work.activate(h.id());
        }
        self.focus = focus;
    }

    /// The keyboard goes to the work area's active pane.
    pub(crate) fn focus_work(&mut self) {
        self.set_focus(Focus::on(self.work.active().unwrap_or(NO_PANE)));
    }

    /// The region the focus is in, as the shell knows them.
    pub(crate) fn region(&self) -> Region {
        match self.focus {
            Focus::Rail => Region::Rail,
            Focus::List => Region::List,
            Focus::Pane(_) => Region::Work,
        }
    }

    /// The focus goes to `region` (the work area: its active pane).
    pub(crate) fn focus_region(&mut self, region: Region) {
        match region {
            Region::Rail => self.set_focus(Focus::Rail),
            Region::List => self.set_focus(Focus::List),
            Region::Work => self.focus_work(),
        }
    }

    /// Move the focus to the next (`1`) or previous (`-1`) panel: rail, list, main pane, thread
    /// panel, round again. A hidden list or a closed pane is skipped. The rail is expanded only
    /// while it has the focus, so passing it never leaves it open.
    pub(super) fn cycle(&mut self, step: isize) {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Stop {
            Rail,
            List,
            Main,
            Thread,
        }
        let mut stops = vec![Stop::Rail];
        if !self.shell.list_hidden {
            stops.push(Stop::List);
        }
        let (main, thread) = (self.work.main_id(), self.work.thread_id());
        stops.extend(main.map(|_| Stop::Main).into_iter().chain(thread.map(|_| Stop::Thread)));
        let here = match self.focus() {
            Focus::Rail => Stop::Rail,
            Focus::List => Stop::List,
            Focus::Pane(h) if Some(h.id()) == thread => Stop::Thread,
            Focus::Pane(_) => Stop::Main,
        };
        let at = stops.iter().position(|s| *s == here);
        let n = stops.len() as isize;
        let to = match at {
            Some(i) => stops[(i as isize + step).rem_euclid(n) as usize],
            None => stops[0],
        };
        self.work.set_insert(false);
        self.set_focus(match to {
            Stop::Rail => Focus::Rail,
            Stop::List => Focus::List,
            Stop::Main => Focus::on(main.unwrap_or(NO_PANE)),
            Stop::Thread => Focus::on(thread.unwrap_or(NO_PANE)),
        });
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
        if let (Some(main), true) = (self.work.main_id(), self.work.active() == self.work.thread_id()) {
            return self.set_focus(Focus::on(main));
        }
        if !self.shell.list_hidden {
            self.focus_list(main);
        }
    }
}
