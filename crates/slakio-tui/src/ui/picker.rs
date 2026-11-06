//! The list of a message's links to pick (`o`, when one of them is not on screen): each row its
//! letters (faint once the letters typed do not start them), the host it goes to (its end kept
//! when cut) and its text after it; the cursor row drawn as the selection. A Slack link, which
//! opens inside slakio, is marked `↳` before its host and named when its text is not. It scrolls with its
//! cursor; when it has more rows than it shows, its title says where the cursor is (`3/30`).
//!
//! ```text
//! ╭ Open which link? ─────────────────────────────╮
//! │ a  docs.example.com  the runbook              │
//! │ s  status.example.com                         │
//! │ d  ↳ acme.slack.com  Slack message            │
//! ╰──────────────── j/k move · Enter open · Esc cancel ─╯
//! ```

use super::modal;
use crate::app::App;
use crate::app::picker::list_rect;
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

/// The mark of a Slack link, which opens in slakio rather than the browser.
pub const INSIDE: &str = "↳ ";

pub(super) fn draw(f: &mut Frame, app: &App, body: Rect) {
    let t = &app.theme;
    let Some(p) = app.picker() else { return };
    let Some(cursor) = p.list else { return };
    let rect = list_rect(body, p.picks.len());
    let label = |l: Label| app.i18n.label(l).to_string();
    let footer =
        format!("j/k {} · Enter {} · Esc {}", label(Label::HintMove), label(Label::HintOpen), label(Label::HintCancel));
    // Rows past the box: where the cursor is among them all.
    let rows = usize::from(crate::screen::inner(rect).height);
    let mut title = label(Label::OpenListTitle);
    if p.picks.len() > rows {
        title = format!("{title} {}/{}", cursor + 1, p.picks.len());
    }
    let inner = modal(f, app, rect, &title, &footer);
    let w = usize::from(inner.width);
    let top = p.list_top(rows);
    let lead_w = p.picks.iter().map(|k| width(&k.hint)).max().unwrap_or(0) + 2;
    for (r, (i, k)) in p.picks.iter().enumerate().skip(top).take(rows).enumerate() {
        let row = Rect { y: inner.y + r as u16, height: 1, ..inner };
        let hint = format!(" {:<w$} ", k.hint, w = lead_w - 2);
        // Labels the letters typed do not start are faint.
        let on = !k.hint.is_empty() && k.hint.starts_with(&p.typed);
        let lead = lead_w as u16;
        // A Slack link opens here, not in the browser: marked, and named when its text is not.
        let slack = crate::permalink::parse(&k.url);
        let mark = if slack.is_some() { INSIDE } else { "" };
        let host = host_tail(&k.host, HOST_MAX.min(w.saturating_sub(lead_w + 2 + width(mark))));
        let room = w.saturating_sub(lead_w + 1 + width(mark) + width(&host) + 2);
        let named = slack.map(|l| {
            let what = if l.message.is_some() { Label::OpenSlackMessage } else { Label::OpenSlackConversation };
            label(what)
        });
        let said = k.label.clone().or(named).map(|l| clip(&l, room)).unwrap_or_default();
        let line = Line::from(vec![
            Span::styled(hint, if on { t.hint() } else { t.faint() }),
            Span::raw(" "),
            Span::styled(mark, t.accent()),
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
