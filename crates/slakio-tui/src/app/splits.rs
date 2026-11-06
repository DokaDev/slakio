//! Splits as the app routes them: what a split opens and where the keyboard goes, the room it
//! needs, zoom and even room, and moving the keyboard to the pane on a side.
//!
//! | what | keys | the keyboard goes to |
//! |---|---|---|
//! | split beside / under | `v` / `s` on a pane, `v` on the list, `Space w v` / `Space w s`, `:vsplit` / `:split` | the new pane |
//! | the pane on a side | `Ctrl+H/J/K/L`, `Space w h/j/k/l`, `h` / `l` on a pane; `Tab` / `F6` go round | that pane (from the leftmost, `h` goes to the list) |
//! | zoom, or every pane again | `z`, `Space w z`, `:zoom` | stays |
//! | even room | `Space w =`, `:equalize` | stays |
//! | close | `Ctrl+W` (the pane, then its tab) | the pane before it |
//!
//! A split of a pane opens the selected message's thread beside it (focused instead where it is
//! open already); with no message selected, the pane's own conversation again (a second view of
//! it, with its own selection). A split of the thread panel, or toward the thread the panel
//! shows, splits the panel off as a pane of its own. From the list, the conversation under its
//! cursor opens beside the pane it opens into. A pane split beside while its thread panel is
//! open would leave the panel no room to show: the panel splits off first, and the three share
//! the room. A split that would leave a pane under [`screen::PANE_MIN_WIDTH`] ×
//! [`screen::PANE_MIN_HEIGHT`] is refused with a notice; one that is done drops that notice.

use super::{App, Focus};
use crate::action::{Action, PaneAction};
use crate::keymap::hints::key_label;
use crate::screen;
use ratatui::layout::Rect;
use slakio_core::i18n::{Label, Msg};
use slakio_core::layout::{Area, Dir, PaneId, Toward, toward};
use slakio_core::model::Target;
use std::time::Instant;

/// What a split opens.
enum Opens {
    /// A target, in a new pane.
    Target(Target),
    /// The thread panel (this pane), as a pane of its own.
    Panel(PaneId),
}

impl App {
    /// Split the pane with the keyboard (from the list, the pane it opens into) along `dir`.
    pub(super) fn split(&mut self, dir: Dir, now: Instant) {
        let (at, opens) = match self.focus() {
            Focus::List => {
                let Some(t) = self.shell.target_at_cursor(&self.model) else {
                    return self.info(Msg::Label(Label::StatusSplitNothing), now);
                };
                match self.work.home_id() {
                    Some(home) if self.work.showing(&t).is_none() => (home, Opens::Target(t)),
                    // Open already, or nothing to split: as `Enter` opens it.
                    _ => return self.open(t, true),
                }
            }
            Focus::Pane(h) => {
                let id = h.id();
                if let Some(owner) = self.work.owner(id) {
                    (owner, Opens::Panel(id))
                } else if let Some(t) = self.work.selected_thread() {
                    let panel = self.work.pane(id).and_then(|p| p.thread);
                    match self.work.showing(&t) {
                        Some(p) if Some(p) == panel => (id, Opens::Panel(p)),
                        Some(p) => return self.set_focus(Focus::on(p)),
                        None => (id, Opens::Target(t)),
                    }
                } else {
                    let Some(t) = self.work.pane(id).map(|p| p.target.clone()) else { return };
                    (id, Opens::Target(t))
                }
            }
            Focus::ViewSwitcher | Focus::Work => return self.info(Msg::Label(Label::StatusSplitNothing), now),
        };
        // Beside a pane with its thread panel, the panel splits off first: three share the room.
        let unit = self.work.unit(at);
        let parts = if dir == Dir::Row && matches!(opens, Opens::Target(_)) { unit.len() + 1 } else { 2 };
        if !self.room(&unit, dir, parts) {
            return self.warn(Msg::Label(Label::StatusSplitRoom), now);
        }
        let new = match opens {
            Opens::Target(t) => self.work.split(at, dir, t),
            Opens::Panel(p) => self.work.detach(p, dir),
        };
        if let Some(id) = new {
            self.set_focus(Focus::on(id));
            self.done(now);
        }
    }

    /// A split, a zoom or a move that was done: a notice that one could not be is no longer true.
    fn done(&mut self, now: Instant) {
        self.status.forget(now, |m| {
            matches!(
                m,
                Msg::Label(
                    Label::StatusSplitRoom
                        | Label::StatusSplitNothing
                        | Label::StatusZoomOne
                        | Label::StatusNoPaneAbove
                        | Label::StatusNoPaneBelow
                )
            )
        });
    }

    /// The panes of the tab shown as laid out in `w` when every pane shows (zoomed or not).
    fn laid_out(&self, w: Rect) -> Vec<(PaneId, Area)> {
        let Some(root) = self.work.layout() else { return Vec::new() };
        root.solve(Area { x: w.x, y: w.y, width: w.width, height: w.height }, self.work.active())
    }

    /// The panes `unit` lay out in room for `parts` along `dir`, each keeping its least.
    fn room(&self, unit: &[PaneId], dir: Dir, parts: usize) -> bool {
        let laid = self.laid_out(self.plain_work());
        let rects = laid.iter().filter(|(id, _)| unit.contains(id)).map(|(_, a)| *a);
        let (lo, hi) = rects.fold((u16::MAX, 0), |(lo, hi), a| match dir {
            Dir::Row => (lo.min(a.x), hi.max(a.x + a.width)),
            Dir::Column => (lo.min(a.y), hi.max(a.y + a.height)),
        });
        let least = if dir == Dir::Row { screen::PANE_MIN_WIDTH } else { screen::PANE_MIN_HEIGHT };
        usize::from(hi.saturating_sub(lo)) >= parts * usize::from(least)
    }

    /// The keyboard to the pane on side `way` of the one that has it (a zoomed tab shows every
    /// pane again): of those facing it, the one it was on last. A pane's own thread panel left
    /// out for room (and back, its pane) comes first to the right (left). `false` when no pane
    /// has the keyboard or none is that way.
    pub(super) fn focus_toward(&mut self, way: Toward, now: Instant) -> bool {
        let Focus::Pane(h) = self.focus() else { return false };
        let from = h.id();
        let laid = self.laid_out(self.areas().work);
        let own = match way {
            Toward::Right => self.work.pane(from).and_then(|p| p.thread),
            Toward::Left => self.work.owner(from),
            Toward::Up | Toward::Down => None,
        };
        let recent = |id| self.work.pane(id).map_or(0, |p| p.focused_at);
        let to = own.filter(|o| !laid.iter().any(|(id, _)| id == o)).or_else(|| toward(&laid, from, way, recent));
        let Some(to) = to else { return false };
        self.set_focus(Focus::on(to));
        self.done(now);
        true
    }

    /// `Ctrl+K` / `Ctrl+J`: the pane above or below, else say there is none.
    pub(super) fn focus_vertical(&mut self, up: bool, now: Instant) {
        if !self.focus_toward(if up { Toward::Up } else { Toward::Down }, now) {
            self.info(Msg::Label(if up { Label::StatusNoPaneAbove } else { Label::StatusNoPaneBelow }), now);
        }
    }

    pub(super) fn zoom(&mut self, now: Instant) {
        match self.work.toggle_zoom() {
            Some(true) => {
                self.done(now);
                let action = Action::Pane(PaneAction::Zoom);
                let keys = key_label(&self.keymap, action, self.region_context()).unwrap_or_else(|| ":zoom".into());
                self.info(Msg::StatusZoomed { keys }, now);
            }
            Some(false) => self.done(now),
            None => self.only_one(now),
        }
    }

    /// `Space w =`: the panes share the room evenly.
    pub(super) fn equalize(&mut self, now: Instant) {
        match self.work.equalize() {
            Some(true) => self.done(now),
            Some(false) => self.info(Msg::Label(Label::StatusZoomOne), now),
            None => self.only_one(now),
        }
    }

    /// Nothing to zoom or even: no tab, or one pane.
    fn only_one(&mut self, now: Instant) {
        let l = if self.work.tabs().is_empty() { Label::StatusNoTab } else { Label::StatusZoomOne };
        self.info(Msg::Label(l), now);
    }
}
