//! The work area: what the list opened, titled with the workspace stripe and the
//! conversation's name. Message history arrives in a later build; until then the body says so.

use super::frame;
use crate::app::App;
use crate::app::shell::Region;
use crate::screen;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use slakio_core::i18n::Label;

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let open = app.shell.open.as_ref().and_then(|target| app.model.target(target));
    let title = match open {
        Some(c) => {
            let ws = app.model.workspaces().iter().find(|w| w.id == c.workspace);
            let stripe = ws.map(|w| Span::styled("▌", t.workspace(w.color))).unwrap_or_default();
            Line::from(vec![Span::raw(" "), stripe, Span::styled(format!("{} ", breadcrumb(c)), t.title)])
        }
        None => Line::default(),
    };
    f.render_widget(frame(app, Region::Work, title), area);
    let body = if open.is_some() { Label::PaneNoHistory } else { Label::PaneEmpty };
    let p = Paragraph::new(app.i18n.label(body).to_string()).style(t.muted).wrap(Wrap { trim: true });
    f.render_widget(p, screen::inner(area));
}

/// How a conversation is named in titles and the status line: `#backend`, `@Minsu`.
pub(super) fn breadcrumb(c: &slakio_core::model::Conversation) -> String {
    let name = c.name.line();
    if c.is_dm() { format!("@{name}") } else { format!("#{name}") }
}
