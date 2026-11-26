//! What is under the mouse, drawn apart from the selection and quieter than it
//! ([`crate::theme::Theme::paint_hover`]): a message of a pane, a row of the list. The selection
//! keeps its own look; a message selected, or one in a range being made, is not hovered.

use crate::app::App;
use crate::app::pane::Hit;
use ratatui::Frame;
use ratatui::layout::Rect;

/// Paint the message under the pointer in a pane's messages `area`: `hits` say which message
/// each row drawn is of, `rows` are the rows a selection paints, with their message. A message
/// `selected` stays as it is.
pub(super) fn message(
    f: &mut Frame,
    app: &App,
    area: Rect,
    hits: &[Hit],
    rows: &[(usize, Rect)],
    selected: impl Fn(usize) -> bool,
) {
    let Some(at) = app.hover_at().filter(|a| area.contains(*a)) else { return };
    let Some(i) = hits.iter().find(|h| h.y == at.y).map(|h| h.message) else { return };
    if selected(i) {
        return;
    }
    for (_, row) in rows.iter().filter(|(j, _)| *j == i) {
        app.theme.paint_hover(f.buffer_mut(), *row);
    }
}

/// Paint a row of the list under the pointer.
pub(super) fn row(f: &mut Frame, app: &App, row: Rect) {
    app.theme.paint_hover(f.buffer_mut(), row);
}
