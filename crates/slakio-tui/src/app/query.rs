//! What the app shows, asked from outside: where the keyboard is, which popup is up, the panes
//! and what they hold, the list panel and the rail, the keyboard help. Tests read the app through
//! these queries, never through the sub-states (which are private to the crate), so the state
//! can change shape without touching them. Panes are named by a [`PaneHandle`], never by their
//! place, so the queries hold when the work area gains splits.
//!
//! A few drivers at the end put the app in a state keys would take long to reach; they are for
//! tests only and hidden from the docs.

use super::App;
use super::composer::Composer;
use super::dialog::Question;
use super::help;
use super::model::Row;
use super::pane::{Pane, Shown};
use super::shell::{Region, View};
use super::timelines::Timeline;
use crate::keymap::Ctx;
use crate::screen::Slot;
use ratatui::layout::Rect;
use slakio_core::model::Target;

/// An open pane, as long as it stays open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaneHandle(Slot);

/// Where the keyboard is, under any popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Rail,
    List,
    Pane(PaneHandle),
}

/// What a pane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneKind {
    Conversation,
    Thread,
}

/// The popup on top, which takes the keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    Dialog(Question),
    Help,
    Palette,
}

/// A pane, read only.
#[derive(Clone, Copy, Debug)]
pub struct PaneRef<'a> {
    pane: &'a Pane,
    handle: PaneHandle,
    timeline: &'a Timeline,
    draft: &'a Composer,
}

impl<'a> PaneRef<'a> {
    pub fn handle(self) -> PaneHandle {
        self.handle
    }

    pub fn kind(self) -> PaneKind {
        if self.pane.is_thread() { PaneKind::Thread } else { PaneKind::Conversation }
    }

    pub fn target(self) -> &'a Target {
        &self.pane.target
    }

    /// The loaded messages, oldest first.
    pub fn messages(self) -> &'a [Shown] {
        &self.timeline.items
    }

    /// The oldest message is loaded.
    pub fn complete(self) -> bool {
        self.timeline.complete
    }

    /// The selected message (index into [`Self::messages`]).
    pub fn selected(self) -> Option<usize> {
        self.pane.selected_index(self.timeline)
    }

    /// The selected messages: the VISUAL range, or the selected one.
    pub fn range(self) -> Option<(usize, usize)> {
        self.pane.range(self.timeline)
    }

    /// The rows drawn last, top down: (message, screen row) for each row of a message.
    pub fn drawn_rows(self) -> Vec<(usize, u16)> {
        self.pane.hits.borrow().iter().map(|h| (h.message, h.y)).collect()
    }

    /// What its composer holds.
    pub fn composer_text(self) -> &'a str {
        self.draft.text()
    }
}

impl App {
    fn pane_ref(&self, slot: Slot) -> Option<PaneRef<'_>> {
        let pane = self.work.pane(slot)?;
        let (timeline, draft) = (self.work.timeline(pane), self.work.draft(pane));
        Some(PaneRef { pane, handle: PaneHandle(slot), timeline, draft })
    }

    /// Where the keyboard is, under any popup.
    pub fn focus(&self) -> Focus {
        match self.shell.focus {
            Region::Rail => Focus::Rail,
            Region::List => Focus::List,
            Region::Work => Focus::Pane(PaneHandle(self.work.side.slot())),
        }
    }

    /// The pane `handle` names, while it is open.
    pub fn pane_for(&self, handle: PaneHandle) -> Option<PaneRef<'_>> {
        self.pane_ref(handle.0)
    }

    /// The pane with the keyboard, if a pane has it.
    pub fn focused_pane(&self) -> Option<PaneRef<'_>> {
        match self.focus() {
            Focus::Pane(h) => self.pane_for(h),
            _ => None,
        }
    }

    /// Every open pane, shown or not, in reading order.
    pub fn open_panes(&self) -> Vec<PaneRef<'_>> {
        [Slot::Main, Slot::Thread].into_iter().filter_map(|s| self.pane_ref(s)).collect()
    }

    /// The panes on screen and where each is drawn (a narrow screen leaves some out).
    pub fn panes_on_screen(&self) -> Vec<(PaneRef<'_>, Rect)> {
        let frame = self.frame();
        frame.panes.iter().filter_map(|l| self.pane_ref(l.slot).map(|p| (p, l.rect))).collect()
    }

    /// Where the messages of pane `handle` are drawn, while it is on screen.
    pub fn message_area(&self, handle: PaneHandle) -> Option<Rect> {
        self.frame().pane(handle.0).map(|l| l.parts.messages)
    }

    /// The open pane that shows `target`.
    pub fn pane_showing(&self, target: &Target) -> Option<PaneRef<'_>> {
        self.open_panes().into_iter().find(|p| p.target() == target)
    }

    /// The conversation open in the work area.
    pub fn open_target(&self) -> Option<&Target> {
        self.pane_ref(Slot::Main).map(PaneRef::target)
    }

    /// The rows of the list panel, top down.
    pub fn list_rows(&self) -> Vec<Row> {
        self.shell.rows(&self.model)
    }

    /// The row of the list panel's cursor.
    pub fn list_cursor(&self) -> usize {
        self.shell.list_cursor
    }

    /// The first row the list panel shows.
    pub fn list_top(&self) -> usize {
        self.shell.list_top
    }

    /// The user hid the list panel.
    pub fn list_hidden(&self) -> bool {
        self.shell.list_hidden
    }

    /// What the list panel lists.
    pub fn view(&self) -> View {
        self.shell.view
    }

    /// The workspace shown (index into the model's workspaces).
    pub fn workspace(&self) -> usize {
        self.shell.workspace
    }

    /// The rail item under the rail's cursor.
    pub fn rail_cursor(&self) -> usize {
        self.shell.rail_cursor
    }

    /// The rail is drawn wide, with labels.
    pub fn rail_expanded(&self) -> bool {
        self.shell.rail_expanded()
    }

    /// The rows of the keyboard help, while it is open (else none).
    pub fn help_rows(&self) -> Vec<help::Row> {
        self.help.as_ref().map(|h| h.rows(&self.keymap, &self.i18n)).unwrap_or_default()
    }

    /// Where the keyboard was when the help opened, while it is open.
    pub fn help_origin(&self) -> Option<Ctx> {
        self.help.as_ref().map(|h| h.origin)
    }

    /// The help's filter is being typed.
    pub fn help_typing(&self) -> bool {
        self.help.as_ref().is_some_and(|h| h.typing)
    }

    /// Test driver: select message `index` (the last one if past it) of the focused pane, as a
    /// click does but without asking for older messages.
    #[doc(hidden)]
    pub fn select_message(&mut self, index: usize) {
        if self.shell.focus != Region::Work {
            return;
        }
        let Some(p) = self.work.focused() else { return };
        let tl = self.work.timeline(p);
        let Some(ts) = tl.items.len().checked_sub(1).map(|last| tl.items[index.min(last)].ts) else { return };
        if let Some(p) = self.work.focused_mut() {
            p.selected = Some(ts);
        }
    }

    /// Test driver: show `target` in a second pane beside the conversation, with the keyboard
    /// (the work area offers no such split yet; two panes on one target share its messages).
    #[doc(hidden)]
    pub fn open_beside(&mut self, target: Target) {
        self.shell.focus = Region::Work;
        self.work.show_beside(target);
    }

    /// Test driver: put the list panel's cursor on row `row` (the last one if past it).
    #[doc(hidden)]
    pub fn move_list_cursor_to(&mut self, row: usize) {
        let last = self.list_rows().len().saturating_sub(1);
        self.shell.list_cursor = row.min(last);
    }

    /// Test driver: the list panel gets the keyboard, its cursor on row `row`.
    #[doc(hidden)]
    pub fn focus_list_row(&mut self, row: usize) {
        self.shell.focus = Region::List;
        self.move_list_cursor_to(row);
    }

    /// Test driver: the list panel lists `view` with every section unfolded.
    #[doc(hidden)]
    pub fn show_unfolded(&mut self, view: View) {
        self.shell.view = view;
        self.shell.collapsed.clear();
    }
}
