//! The list panel: the view picked on the rail. Home lists the workspace's sidebar sections
//! (folded with `Enter`) and their conversations; DMs lists the DMs. Views built in a later
//! version say so instead of showing anything.
//!
//! ```text
//! ╭ Home ────────────╮
//! │▾ Favorites       │
//! │ # backend      3 │   3 = mentions
//! │ # incidents    ● │   ● = unread
//! │ ▪ secret-proj    │   ▪ = private
//! │ # partner ⇄      │   ⇄ = shared with another organization
//! │ @ Minsu        2 │
//! ```

use super::{frame, highlight, view_label};
use crate::app::App;
use crate::app::model::Row;
use crate::app::shell::Region;
use crate::screen;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use slakio_core::i18n::Label;
use slakio_core::model::ConversationKind;

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let title = Line::from(format!(" {} ", app.i18n.label(view_label(app.shell.view))));
    f.render_widget(frame(app, Region::List, title), area);
    let inner = screen::inner(area);
    let rows = app.shell.rows(&app.model);
    let note = if !app.model.is_loaded() {
        Some(Label::ListLoading)
    } else if app.shell.view.is_placeholder() {
        Some(Label::ListLaterVersion)
    } else if rows.is_empty() {
        Some(Label::ListEmpty)
    } else {
        None
    };
    if let Some(l) = note {
        let p = Paragraph::new(app.i18n.label(l).to_string()).style(t.muted).wrap(Wrap { trim: true });
        f.render_widget(p, inner);
        return;
    }
    let focused = app.shell.focus == Region::List;
    for (k, &row) in rows.iter().enumerate().skip(app.shell.list_top).take(usize::from(inner.height)) {
        let y = inner.y + (k - app.shell.list_top) as u16;
        let area = Rect { y, height: 1, ..inner };
        draw_row(f, app, row, area);
        if k == app.shell.list_cursor {
            highlight(f, area, if focused { t.cursor } else { t.cursor_inactive });
        }
    }
}

fn draw_row(f: &mut Frame, app: &App, row: Row, area: Rect) {
    let t = &app.theme;
    match row {
        Row::Section(i) => {
            let s = app.model.section(i);
            let fold = if app.shell.collapsed.contains(&s.id) { "▸" } else { "▾" };
            f.render_widget(Paragraph::new(format!("{fold} {}", s.name.line())).style(t.section), area);
        }
        Row::Conversation(i) => {
            let c = app.model.conversation(i);
            let prefix = match c.kind {
                ConversationKind::Channel { private: false } => "#",
                ConversationKind::Channel { private: true } => "▪",
                ConversationKind::Dm { .. } | ConversationKind::GroupDm { .. } => "@",
            };
            let unread = c.unread > 0 && !c.muted;
            let name_style = if c.muted {
                t.muted_conversation
            } else if unread {
                t.unread
            } else {
                t.text
            };
            let (badge, badge_style) = if c.mentions > 0 {
                (c.mentions.to_string(), t.mention)
            } else if unread && c.is_dm() {
                (c.unread.to_string(), t.mention)
            } else if unread {
                ("●".to_string(), t.unread)
            } else {
                (String::new(), t.text)
            };
            let badge_w = Span::raw(badge.as_str()).width() as u16;
            let name_w = area.width.saturating_sub(badge_w + 2);
            let mut spans = vec![
                Span::styled(format!(" {prefix} "), name_style),
                Span::styled(c.name.line().into_string(), name_style),
            ];
            if c.external {
                spans.push(Span::styled(" ⇄", t.muted));
            }
            f.render_widget(Paragraph::new(Line::from(spans)), Rect { width: name_w, ..area });
            if badge_w > 0 {
                let x = area.right().saturating_sub(badge_w + 1);
                f.render_widget(Paragraph::new(badge).style(badge_style), Rect { x, width: badge_w, ..area });
            }
        }
    }
}
