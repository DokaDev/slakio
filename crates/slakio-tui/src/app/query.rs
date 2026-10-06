//! What the app shows, asked from outside: where the keyboard is, which popup is up, the panes
//! and what they hold. Tests read the app through these queries, never
//! through the sub-states, so the work area can change shape without touching them.

use super::App;
use super::dialog::Question;
use super::pane::{Pane, Shown};
use super::shell::Region;
use super::work::Side;
use crate::screen::Slot;
use ratatui::layout::Rect;
use slakio_core::model::Target;

/// Where the keyboard is, under any popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Rail,
    List,
    /// The pane with a conversation.
    Conversation,
    /// The thread panel.
    Thread,
}

impl Focus {
    /// A pane of the work area has the keyboard.
    pub fn is_pane(self) -> bool {
        matches!(self, Self::Conversation | Self::Thread)
    }
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
pub struct PaneRef<'a>(&'a Pane);

impl<'a> PaneRef<'a> {
    pub fn kind(self) -> PaneKind {
        if self.0.is_thread() { PaneKind::Thread } else { PaneKind::Conversation }
    }

    pub fn target(self) -> &'a Target {
        &self.0.target
    }

    /// The loaded messages, oldest first.
    pub fn messages(self) -> &'a [Shown] {
        &self.0.items
    }

    /// The oldest message is loaded.
    pub fn complete(self) -> bool {
        self.0.complete
    }

    /// The selected message (index into [`Self::messages`]).
    pub fn selected(self) -> Option<usize> {
        self.0.selected
    }

    /// The selected messages: the VISUAL range, or the selected one.
    pub fn range(self) -> Option<(usize, usize)> {
        self.0.range()
    }

    /// The rows drawn last, top down: (message, screen row) for each row of a message.
    pub fn drawn_rows(self) -> Vec<(usize, u16)> {
        self.0.hits.borrow().iter().map(|h| (h.message, h.y)).collect()
    }

    /// What its composer holds.
    pub fn composer_text(self) -> &'a str {
        self.0.composer.text()
    }
}

impl App {
    /// Where the keyboard is, under any popup.
    pub fn focus(&self) -> Focus {
        match (self.shell.focus, self.work.side) {
            (Region::Rail, _) => Focus::Rail,
            (Region::List, _) => Focus::List,
            (Region::Work, Side::Main) => Focus::Conversation,
            (Region::Work, Side::Thread) => Focus::Thread,
        }
    }

    /// The popup that takes the keys, if one is up.
    pub fn overlay(&self) -> Option<Overlay> {
        if let Some(d) = &self.dialog {
            return Some(Overlay::Dialog(d.question));
        }
        if self.help.is_some() {
            return Some(Overlay::Help);
        }
        self.cmdline.is_open().then_some(Overlay::Palette)
    }

    /// The pane that has (or would have, with the work area focused) the keyboard.
    pub fn focused_pane(&self) -> Option<PaneRef<'_>> {
        self.work.focused().map(PaneRef)
    }

    /// The pane of kind `kind`, when open.
    pub fn pane_of(&self, kind: PaneKind) -> Option<PaneRef<'_>> {
        match kind {
            PaneKind::Conversation => self.work.main.as_ref(),
            PaneKind::Thread => self.work.thread.as_ref(),
        }
        .map(PaneRef)
    }

    /// The conversation open in the work area.
    pub fn open_target(&self) -> Option<&Target> {
        self.pane_of(PaneKind::Conversation).map(PaneRef::target)
    }

    /// Where the pane of kind `kind` is drawn; `None` when it is closed or left out (a narrow
    /// screen shows only the pane with the keyboard).
    pub fn pane_area(&self, kind: PaneKind) -> Option<Rect> {
        let slot = match kind {
            PaneKind::Conversation => Slot::Main,
            PaneKind::Thread => Slot::Thread,
        };
        self.frame().pane(slot).map(|p| p.rect)
    }

    /// Select message `index` of the focused pane, as a click does but without asking for
    /// older messages (for tests that pick a message by what it holds).
    pub fn select_message(&mut self, index: usize) {
        if let Some(p) = self.work.focused_mut() {
            p.selected = Some(index);
        }
    }
}
