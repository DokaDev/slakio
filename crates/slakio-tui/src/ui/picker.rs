//! The list of a message's links to pick (`o`, when one of them is not on screen): each row its
//! letters, the host it goes to (its end kept when cut) and its text after it; the cursor row
//! drawn as the selection.
//!
//! ```text
//! ╭ Open which link? ─────────────────────────────╮
//! │ a  docs.example.com  the runbook              │
//! │ s  status.example.com                         │
//! ╰──────────────── j/k move · Enter open · Esc cancel ─╯
//! ```

use super::modal;
use crate::app::App;
use crate::app::links::list_rect;
use crate::opener::host_tail;
use crate::text::{clip, width};
use crate::theme::Selection;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::i18n::Label;

/// The most cells of a row the host takes, so its text shows too.
const HOST_MAX: usize = 32;

pub(super) fn draw(f: &mut Frame, app: &App, body: Rect) {
    let t = &app.theme;
    let Some(p) = app.picker() else { return };
    let Some(cursor) = p.list else { return };
    let rect = list_rect(body, p.picks.len());
    let label = |l: Label| app.i18n.label(l).to_string();
    let footer =
        format!("j/k {} · Enter {} · Esc {}", label(Label::HintMove), label(Label::HintOpen), label(Label::HintCancel));
    let inner = modal(f, app, rect, &label(Label::OpenListTitle), &footer);
    let w = usize::from(inner.width);
    for (i, k) in p.picks.iter().enumerate().take(usize::from(inner.height)) {
        let row = Rect { y: inner.y + i as u16, height: 1, ..inner };
        let hint = format!(" {} ", k.hint);
        let lead = width(&hint) as u16;
        let host = host_tail(&k.host, HOST_MAX.min(w.saturating_sub(width(&hint) + 2)));
        let room = w.saturating_sub(width(&hint) + 1 + width(&host) + 2);
        let said = k.label.as_deref().map(|l| clip(l, room)).unwrap_or_default();
        let line = Line::from(vec![
            Span::styled(hint, t.hint()),
            Span::raw(" "),
            Span::styled(host, t.bold()),
            Span::raw("  "),
            Span::styled(said, t.muted()),
        ]);
        f.render_widget(Paragraph::new(line), row);
        if i == cursor {
            // The letters keep their own look.
            let rest = Rect { x: row.x + lead, width: row.width.saturating_sub(lead), ..row };
            t.paint_selection(f.buffer_mut(), rest, Selection::Focused);
        }
    }
}
