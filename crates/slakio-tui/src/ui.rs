//! Drawing a frame of [`App`]: the work area (empty for now) and the status line.
//!
//! ```text
//!                         slakio
//!   Early development: nothing connects to Slack yet.
//!          Type :qa and press Enter to quit.
//!
//!  NORMAL  <notice>
//! ```

use crate::app::status::Level;
use crate::app::{App, Mode};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::i18n::{Label, Localized};
use std::time::Instant;

/// Draw the whole screen at `now` (notices that ran out are not drawn).
pub fn draw(f: &mut Frame, app: &App, now: Instant) {
    let [body, status] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(f.area());
    draw_body(f, app, body);
    draw_status(f, app, status, now);
}

fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let lines = vec![
        Line::styled(Localized::verbatim("slakio").to_string(), t.title),
        Line::styled(app.i18n.label(Label::WelcomeStatus).to_string(), t.muted),
        Line::styled(app.i18n.label(Label::WelcomeQuitHint).to_string(), t.muted),
    ];
    let height = lines.len() as u16;
    let top = area.y + area.height.saturating_sub(height) / 2;
    let area = Rect { y: top, height: height.min(area.height), ..area };
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center).style(t.text), area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect, now: Instant) {
    let t = &app.theme;
    let (label, style) = match app.mode() {
        Mode::Normal => (Label::ModeNormal, t.mode_normal),
        Mode::Command => (Label::ModeCommand, t.mode_command),
    };
    let badge = format!(" {} ", app.i18n.label(label));
    let badge_width = width(&badge);
    let mut spans = vec![Span::styled(badge, style), Span::raw(" ")];
    let mut cursor = None;
    if app.mode() == Mode::Command {
        let text = format!(":{}", app.cmdline.text());
        let x = area.x + (badge_width + 1 + width(&text)) as u16;
        cursor = Some(Position { x: x.min(area.right().saturating_sub(1)), y: area.y });
        spans.push(Span::styled(text, t.text));
    } else if let Some(n) = app.status.notice(now) {
        let style = match n.level {
            Level::Warning => t.warning,
            Level::Info => t.text,
        };
        spans.push(Span::styled(app.i18n.msg(&n.msg).to_string(), style));
    }
    f.render_widget(Paragraph::new(Line::from(spans)).style(t.status), area);
    if let Some(p) = cursor {
        f.set_cursor_position(p);
    }
}

/// Display width of text in terminal cells.
fn width(s: &str) -> usize {
    Span::raw(s).width()
}
