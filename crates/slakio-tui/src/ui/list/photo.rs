//! A DM's peer's profile photo in the list, in place of the initials chip: the same two cells on
//! the row (the presence mark and the name stay where they are), drawn as a picture's
//! placeholder cells ([`crate::images::graphics`]) with `avatars = "image"` once the terminal
//! holds it. Asked for only while its row is drawn, in the pictures' one queue
//! ([`crate::images::Images::look_as`]); a row scrolled, folded or filtered away, or a list
//! left out, draws no cells, so nothing of the photo stays on screen. Pictures off, not here
//! yet or failed: the initials chip.

use crate::app::App;
use crate::images::decode::Shape;
use crate::images::{CHIP_PHOTO, Look, graphics};
use ratatui::style::Style;
use ratatui::text::Span;
use slakio_core::images::Source;
use slakio_core::model::{UserId, WorkspaceId};

/// The placeholder cells of `user`'s photo (of workspace `ws`) for a DM row, when photos are on
/// and the terminal holds it.
pub(in crate::ui) fn chip(app: &App, ws: &WorkspaceId, user: &UserId) -> Option<Span<'static>> {
    if !app.settings.photos {
        return None;
    }
    let address = app.model.user(ws, user)?.avatar.clone()?;
    match app.images.look_as(ws, &Source::Avatar(address), CHIP_PHOTO) {
        Look::Ready { id, cols, rows } if CHIP_PHOTO == (Shape::Photo { cols, rows }) => {
            Some(Span::styled(graphics::row(0, cols), Style::default().fg(graphics::color(id))))
        }
        _ => None,
    }
}
