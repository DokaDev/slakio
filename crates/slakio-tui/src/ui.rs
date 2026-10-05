//! Drawing a frame of [`App`]. Without a backend: the welcome text and the status line. With
//! one: the shell — rail, list panel, work area — over the status line (see [`crate::screen`]
//! for the geometry), or, on a terminal too small for it, only how much room it needs.
//!
//! * [`rail`], [`list`], [`work`] — the three regions; [`timeline`] — a pane's messages;
//!   [`statusline`] — the bottom line.
//!
//! Names and messages drawn here are remote text ([`slakio_core::sanitize::Remote`]): it can
//! only be drawn through the sanitiser (`line()`, `block()`), never as it came. A test keeps
//! `Remote::unsanitized` out of this module.

mod list;
mod rail;
mod statusline;
pub mod timeline;
mod work;

use crate::app::App;
use crate::app::shell::{Region, View};
use crate::screen;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::symbols::border;
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use slakio_core::i18n::{Label, Localized, Msg};
use slakio_core::sanitize::Safe;
use std::time::Instant;

/// Draw the whole screen at `now` (notices that ran out are not drawn).
pub fn draw(f: &mut Frame, app: &App, now: Instant) {
    let area = f.area();
    if app.backend.is_none() {
        let [body, status] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
        draw_welcome(f, app, body);
        statusline::draw(f, app, status, now);
        return;
    }
    if screen::too_small(area) {
        draw_too_small(f, app, area);
        return;
    }
    let a = app.areas();
    if let Some(l) = a.list {
        list::draw(f, app, l);
    }
    work::draw(f, app, a.work);
    // The expanded rail over the list panel hides all of it: what would show beside the rail
    // (a piece of the border, cut names) is cleared, not left half drawn.
    if let Some(l) = a.list.filter(|l| l.intersects(a.rail)) {
        f.render_widget(Clear, l);
    }
    rail::draw(f, app, a.rail);
    statusline::draw(f, app, a.status, now);
}

fn draw_welcome(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let lines = vec![
        Line::styled(Localized::verbatim("slakio").to_string(), t.title),
        Line::styled(app.i18n.label(Label::WelcomeStatus).to_string(), t.muted),
        Line::styled(app.i18n.label(Label::WelcomeQuitHint).to_string(), t.muted),
        Line::styled(app.i18n.label(Label::WelcomeDemoHint).to_string(), t.muted),
    ];
    let height = lines.len() as u16;
    let top = area.y + area.height.saturating_sub(height) / 2;
    let area = Rect { y: top, height: height.min(area.height), ..area };
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center).style(t.text), area);
}

fn draw_too_small(f: &mut Frame, app: &App, area: Rect) {
    let msg = Msg::ScreenTooSmall {
        width: area.width.to_string(),
        height: area.height.to_string(),
        min_width: screen::MIN_WIDTH.to_string(),
        min_height: screen::MIN_HEIGHT.to_string(),
    };
    let text = app.i18n.msg(&msg).to_string();
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }).style(app.theme.muted), area);
}

/// A region's frame: rounded, its border marking the focus.
fn frame<'a>(app: &App, region: Region, title: Line<'a>) -> Block<'a> {
    let t = &app.theme;
    let style = if app.shell.focus == region { t.border_focus } else { t.border };
    Block::bordered().border_set(border::ROUNDED).border_style(style).title(title)
}

/// The name of a view (rail labels, list title, breadcrumb).
fn view_label(view: View) -> Label {
    match view {
        View::Home => Label::RailHome,
        View::Dms => Label::RailDms,
        View::Activity => Label::RailActivity,
        View::Files => Label::RailFiles,
        View::Later => Label::RailLater,
    }
}

/// The rail's letter, or its Nerd Font icon, for a view.
fn view_glyph(view: View, icons: bool) -> &'static str {
    match (view, icons) {
        (View::Home, false) => "H",
        (View::Dms, false) => "D",
        (View::Activity, false) => "A",
        (View::Files, false) => "F",
        (View::Later, false) => "L",
        (View::Home, true) => "\u{F02DC}",
        (View::Dms, true) => "\u{F0361}",
        (View::Activity, true) => "\u{F009A}",
        (View::Files, true) => "\u{F0219}",
        (View::Later, true) => "\u{F00C0}",
    }
}

/// The first letter of a workspace's name, upper case: its rail letter.
fn workspace_letter(name: &Safe) -> String {
    name.as_str().chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default()
}

/// Paint `style` over a whole row (the cursor).
fn highlight(f: &mut Frame, row: Rect, style: Style) {
    f.buffer_mut().set_style(row, style);
}
