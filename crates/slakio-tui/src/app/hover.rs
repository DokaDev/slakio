//! Where the mouse is, and what is under it: the row of the list or the message of a pane drawn
//! apart from the selection ([`crate::theme::Theme::paint_hover`]).
//!
//! Only a motion (a move, a drag) says where the pointer is: a click or the wheel says nothing
//! of it, so a terminal that reports no motion hovers nothing. The thing under the pointer is
//! drawn again only when it changes (a move within one message draws nothing), and nothing is
//! hovered once the terminal loses the focus, while a popup is up, or on a screen too small.

use super::App;
use super::model::Row;
use crate::screen;
use ratatui::layout::Position;
use slakio_core::layout::PaneId;
use std::time::Instant;

/// The pointer: where the last motion left it, and the last click (for a double click).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Pointer {
    /// Where the last move or drag was; `None` before one and once the focus is lost.
    pub at: Option<Position>,
    /// The last click: when, and where.
    pub last_click: Option<(Instant, Position)>,
}

/// What is under the pointer: a row of the list (the row of an item, for its second line), or
/// a message of a pane. A change of it is what draws the screen again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HoverKey {
    List(usize),
    Message(PaneId, usize),
}

impl App {
    /// Where the pointer hovers: `None` before any motion, after the focus was lost, while a
    /// popup, a picker, the help or the sign-in screen is up, and on a screen too small.
    pub fn hover_at(&self) -> Option<Position> {
        let at = self.pointer.at?;
        let busy = !self.layers(None).is_empty()
            || self.picker.is_some()
            || self.backend.is_none()
            || self.sign_in.state.is_some()
            || screen::too_small(self.size);
        (!busy).then_some(at)
    }

    /// The selectable row of the list under the pointer (an item for its second line).
    pub(crate) fn hovered_list_row(&self) -> Option<usize> {
        let at = self.hover_at()?;
        let rows = self.list_parts()?.rows;
        if !rows.contains(at) {
            return None;
        }
        let row_at = usize::from(at.y - rows.y);
        if row_at >= self.list_height() {
            return None;
        }
        let list = self.shell.rows(&self.model);
        let row = self.shell.list_top + row_at;
        let row = match list.get(row) {
            Some(Row::ItemMeta(_) | Row::NoticeMeta(_)) => row.checked_sub(1)?,
            _ => row,
        };
        list.get(row).is_some_and(|r| r.is_selectable()).then_some(row)
    }

    /// The message of a pane under the pointer, as last drawn.
    fn hovered_message(&self) -> Option<(PaneId, usize)> {
        let at = self.hover_at()?;
        let layout = *self.frame().pane_at(at)?;
        if !layout.parts.messages.contains(at) {
            return None;
        }
        let hit = self.work.pane(layout.id)?.hit(at.y)?;
        Some((layout.id, hit.message))
    }

    /// What is under the pointer now.
    pub(super) fn hover_key(&self) -> Option<HoverKey> {
        self.hovered_list_row()
            .map(HoverKey::List)
            .or_else(|| self.hovered_message().map(|(id, i)| HoverKey::Message(id, i)))
    }

    /// The terminal lost the focus: the pointer is gone with it. `true` when something was
    /// hovered.
    pub(super) fn pointer_gone(&mut self) -> bool {
        let was = self.hover_key().is_some();
        self.pointer.at = None;
        was
    }
}
