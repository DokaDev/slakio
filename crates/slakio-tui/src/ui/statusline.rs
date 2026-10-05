//! The status line, lualine-style: mode badge · workspace · where you are · pending keys or a
//! notice, and on the right the unread totals and the backend's state.
//!
//! ```text
//!  NORMAL  ▌A company  #backend  Space w                          @3  DM 2  demo
//! ```

use super::work::breadcrumb;
use super::{view_glyph, view_label};
use crate::app::shell::View;
use crate::app::status::Level;
use crate::app::{App, Mode};
use crate::keymap::keys;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::i18n::Label;
use std::time::Instant;

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect, now: Instant) {
    let t = &app.theme;
    let (label, style) = match app.mode() {
        Mode::Normal => (Label::ModeNormal, t.mode_normal),
        Mode::Command => (Label::ModeCommand, t.mode_command),
    };
    let badge = format!(" {} ", app.i18n.label(label));
    let badge_width = width(&badge);
    let mut spans = vec![Span::styled(badge, style), Span::raw(" ")];
    if app.mode() == Mode::Command {
        let text = format!(":{}", app.cmdline.text());
        let x = area.x + (badge_width + 1 + width(&text)) as u16;
        spans.push(Span::styled(text, t.text));
        f.render_widget(Paragraph::new(Line::from(spans)).style(t.status), area);
        f.set_cursor_position(Position { x: x.min(area.right().saturating_sub(1)), y: area.y });
        return;
    }
    let right = right_segments(app);
    let right_w = right.iter().map(Span::width).sum::<usize>() as u16;
    if app.backend.is_some() {
        if let Some(ws) = app.model.workspaces().get(app.shell.workspace) {
            spans.push(Span::styled("▌", t.workspace(ws.color)));
            spans.push(Span::styled(format!("{}  ", ws.name.line()), t.text));
        }
        let place = match app.shell.open.as_ref().and_then(|o| app.model.target(o)) {
            Some(c) => breadcrumb(c),
            None => app.i18n.label(view_label(app.shell.view)).to_string(),
        };
        spans.push(Span::styled(format!("{place}  "), t.text));
    }
    if !app.keys.pending().is_empty() {
        spans.push(Span::styled(keys::label(app.keys.pending()), t.title));
    } else if let Some(n) = app.status.notice(now) {
        let style = match n.level {
            Level::Warning => t.warning,
            Level::Info => t.text,
        };
        spans.push(Span::styled(app.i18n.msg(&n.msg).to_string(), style));
    }
    let left = Rect { width: area.width.saturating_sub(right_w), ..area };
    f.render_widget(Paragraph::new(Line::default()).style(t.status), area);
    f.render_widget(Paragraph::new(Line::from(spans)).style(t.status), left);
    if right_w > 0 && right_w < area.width {
        let at = Rect { x: area.right() - right_w, width: right_w, ..area };
        f.render_widget(Paragraph::new(Line::from(right)).style(t.status), at);
    }
}

/// Unread totals (mentions, DMs of this workspace) and the backend's state.
fn right_segments(app: &App) -> Vec<Span<'static>> {
    let t = &app.theme;
    let Some(caps) = app.backend else { return vec![] };
    let mut out = vec![];
    let icons = app.settings.icons;
    let mentions = app.model.mentions();
    if mentions > 0 {
        let text =
            if icons { format!("{} {mentions}  ", view_glyph(View::Activity, true)) } else { format!("@{mentions}  ") };
        out.push(Span::styled(text, t.mention));
    }
    let dms = app.model.dm_unread(app.shell.workspace);
    if dms > 0 {
        let name =
            if icons { view_glyph(View::Dms, true).to_string() } else { app.i18n.label(Label::StatusDms).to_string() };
        out.push(Span::styled(format!("{name} {dms}  "), t.unread));
    }
    if caps.demo {
        out.push(Span::styled(format!("{} ", app.i18n.label(Label::StatusDemo)), t.connection));
    }
    out
}

/// Display width of text in terminal cells.
fn width(s: &str) -> usize {
    Span::raw(s).width()
}
