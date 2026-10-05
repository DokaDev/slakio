//! The work area: the main pane with the conversation the list opened and, beside it, the auto
//! thread panel. Each pane is titled with the workspace stripe and the conversation's name,
//! shows its messages ([`super::timeline`]) and has its own composer at the bottom.
//!
//! ```text
//! ╭ ▌#backend ─────────────────────╮╭ ⤷ Thread · #backend ──╮
//! │ Kim   Starting deploy   10:02  ││ Kim   Starting …       │
//! │╭ Message #backend ────────────╮││╭ Reply ──────────────╮│
//! ││ Press i to write             ││││                      ││
//! │╰──────────────────────────────╯││╰──────────────────────╯│
//! ╰────────────────────────────────╯╰────────────────────────╯
//! ```

use super::{frame, timeline};
use crate::app::App;
use crate::app::pane::Pane;
use crate::app::shell::Region;
use crate::app::work::Side;
use crate::screen;
use crate::text::width;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::Conversation;

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect) {
    let Some(main) = app.work.main.as_ref() else {
        f.render_widget(frame(app, Region::Work, Line::default()), area);
        let p = Paragraph::new(app.i18n.label(Label::PaneEmpty).to_string()).style(app.theme.muted);
        f.render_widget(p.wrap(Wrap { trim: true }), screen::inner(area));
        return;
    };
    let (m, thread) = screen::work_split(area, app.work.thread.is_some());
    draw_pane(f, app, main, m, Side::Main);
    if let (Some(p), Some(r)) = (app.work.thread.as_ref(), thread) {
        draw_pane(f, app, p, r, Side::Thread);
    }
}

fn draw_pane(f: &mut Frame, app: &App, pane: &Pane, area: Rect, side: Side) {
    let t = &app.theme;
    let focused = app.shell.focus == Region::Work && app.work.side == side;
    let conversation = app.model.target(&pane.target);
    let name = conversation.map(breadcrumb).unwrap_or_default();
    let stripe = app
        .model
        .workspace(pane.target.workspace())
        .map(|w| Span::styled("▌", t.workspace(w.color)))
        .unwrap_or_default();
    let title = match side {
        Side::Main => Line::from(vec![Span::raw(" "), stripe, Span::styled(format!("{name} "), t.title)]),
        Side::Thread => {
            let thread = app.i18n.label(Label::PaneThread);
            Line::from(vec![Span::raw(" "), stripe, Span::styled(format!("⤷ {thread} · {name} "), t.title)])
        }
    };
    let style = if focused { t.border_focus } else { t.border };
    f.render_widget(Block::bordered().border_set(border::ROUNDED).border_style(style).title(title), area);
    let inner = screen::inner(area);
    let insert = focused && app.work.insert;
    let view = pane.composer.view(usize::from(inner.width.saturating_sub(2)));
    let lines = (view.lines.len() as u16).clamp(1, screen::COMPOSER_MAX_LINES);
    let composer_h = (lines + 2).min(inner.height.saturating_sub(1));
    let [messages, composer] = Layout::vertical([Constraint::Min(0), Constraint::Length(composer_h)]).areas(inner);
    timeline::draw(f, app, pane, messages, focused);
    if composer.height < 3 {
        return;
    }
    let title = match side {
        Side::Main => app.i18n.msg(&Msg::ComposerMessage { name }).to_string(),
        Side::Thread => app.i18n.label(Label::ComposerReply).to_string(),
    };
    let style = if insert { t.border_focus } else { t.border };
    let block = Block::bordered().border_set(border::ROUNDED).border_style(style).title(format!(" {title} "));
    f.render_widget(block, composer);
    let box_inner = screen::inner(composer);
    if pane.composer.is_empty() && !insert {
        let hint = app.i18n.label(Label::ComposerHint).to_string();
        f.render_widget(Paragraph::new(hint).style(t.muted), box_inner);
        return;
    }
    let h = usize::from(box_inner.height);
    let top = (view.cursor.0 + 1).saturating_sub(h);
    for (k, l) in view.lines.iter().skip(top).take(h).enumerate() {
        let row = Rect { y: box_inner.y + k as u16, height: 1, ..box_inner };
        f.render_widget(Paragraph::new(l.as_str()).style(t.text), row);
    }
    if insert {
        let x = box_inner.x + (view.cursor.1 as u16).min(box_inner.width.saturating_sub(1));
        let y = box_inner.y + (view.cursor.0 - top) as u16;
        f.set_cursor_position(Position { x, y });
    }
    // Width of what is drawn never exceeds the box: the composer wraps at its inner width.
    debug_assert!(view.lines.iter().all(|l| width(l) <= usize::from(box_inner.width.max(2))));
}

/// How a conversation is named in titles and the status line: `#backend`, `@Minsu`.
pub(super) fn breadcrumb(c: &Conversation) -> String {
    let name = c.name.line();
    if c.is_dm() { format!("@{name}") } else { format!("#{name}") }
}
