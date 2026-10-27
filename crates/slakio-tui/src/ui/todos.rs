//! The rows of saved items in the list panel: To-dos' filter row, the group headers (To-dos'
//! In progress and Completed, Home's Saved) and the items. A to-do takes two rows: its author's
//! chip, its text and its reminder, then who wrote it where and when; a saved message in Home
//! takes one.
//!
//! ```text
//! │ All ▾ · Due date ▾           │   the filter, then the order
//! │                              │
//! │ ▾ In progress              6 │   col 1 fold mark, col 3 the group's name, its count (Saved: folded only)
//! │ MK Check the dep…    Overdue │   overdue in the warning color (never a mention's red)
//! │    #backend · 01-05          │   short of room: the place and the date (the chip names the author)
//! │ JP Ship the release   in 20h │   upcoming: muted
//! │ ▾ Completed                2 │
//! │ SL ✓ Review the design       │   completed: muted, checked
//! ```

use super::avatar_chip;
use crate::app::App;
use crate::app::model::{Row, breadcrumb};
use crate::app::shell::View;
use crate::app::todos::{Due, Group};
use crate::avatar;
use crate::text::{clip, width};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::{SavedState, Target};

/// Draw `spans` from column `x` of `row`, at most `w` cells.
fn put(f: &mut Frame, row: Rect, x: u16, w: u16, spans: Vec<Span<'_>>) {
    let x = row.x + x;
    if x >= row.right() || w == 0 {
        return;
    }
    let w = w.min(row.right() - x);
    f.render_widget(Paragraph::new(Line::from(spans)), Rect { x, width: w, ..row });
}

/// `text` at the right of `row`, one cell in from its edge; its width (0 when it does not fit).
fn right(f: &mut Frame, row: Rect, text: &str, style: Style) -> u16 {
    let w = width(text) as u16;
    if w == 0 || w + 2 > row.width {
        return 0;
    }
    put(f, row, row.width - 1 - w, w, vec![Span::styled(text.to_string(), style)]);
    w
}

/// Where an item's chip starts: under a section's name in Home, at the edge in To-dos.
fn item_x(app: &App) -> u16 {
    if app.shell.view == View::Home { 3 } else { 1 }
}

/// Where an item's text starts: after its author's chip (avatars on).
fn text_x(app: &App) -> u16 {
    item_x(app) + if app.settings.avatars { avatar::WIDTH as u16 + 1 } else { 0 }
}

/// Draw `row`, one of the saved items' rows, in `area`.
pub(super) fn draw(f: &mut Frame, app: &App, row: Row, area: Rect) {
    match row {
        Row::Filter => filter(f, app, area),
        Row::Group(g) => group(f, app, g, area),
        Row::Item(i) => item(f, app, i, area),
        Row::ItemMeta(i) => meta(f, app, i, area),
        _ => {}
    }
}

fn filter(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let r = app.filter_row();
    let spans = vec![Span::styled(r.filter, t.text()), Span::styled(" · ", t.faint()), Span::styled(r.sort, t.text())];
    put(f, area, 1, area.width.saturating_sub(2), spans);
}

fn group(f: &mut Frame, app: &App, g: Group, area: Rect) {
    let t = &app.theme;
    let folded = app.shell.groups.contains(&g);
    let n = app.model.group_items(app.shell.workspace, g, &app.shell.todo).len();
    // To-dos' groups always count; Home's Saved, as Home's sections, only when folded.
    let used = if g != Group::Saved || folded { right(f, area, &n.to_string(), t.muted()) } else { 0 };
    put(f, area, 1, 1, vec![Span::styled(if folded { "▸" } else { "▾" }, t.faint())]);
    let name = app.i18n.label(match g {
        Group::Saved => Label::GroupSaved,
        Group::InProgress => Label::GroupInProgress,
        Group::Completed => Label::GroupCompleted,
    });
    let room = area.width.saturating_sub(3 + used + 2);
    put(f, area, 3, room, vec![Span::styled(clip(&name, usize::from(room)), t.section())]);
}

/// The reminder's badge of an item: overdue, or how long until it is due.
fn due_badge(app: &App, i: usize) -> Option<(String, Style)> {
    let t = &app.theme;
    let s = app.model.saved(i);
    match app.model.due(s) {
        Due::Overdue => Some((app.i18n.label(Label::DueOverdue).to_string(), t.warning())),
        Due::Upcoming => {
            let hours = s.due?.0.saturating_sub(app.model.taken().0) / 3_600_000_000;
            let msg = if hours < 24 {
                Msg::DueInHours { hours: hours.max(1).to_string() }
            } else {
                Msg::DueInDays { days: (hours / 24).to_string() }
            };
            Some((app.i18n.msg(&msg).to_string(), t.muted()))
        }
        Due::Past | Due::None => None,
    }
}

fn item(f: &mut Frame, app: &App, i: usize, area: Rect) {
    let t = &app.theme;
    let s = app.model.saved(i);
    let author = app.model.user(&s.author).map(|u| u.display_name.line().into_string()).unwrap_or_default();
    if let Some(chip) = avatar_chip(app, &s.author, &author) {
        put(f, area, item_x(app), avatar::WIDTH as u16, vec![chip]);
    }
    let badge = if s.state == SavedState::InProgress { due_badge(app, i) } else { None };
    let used = badge.map_or(0, |(text, style)| right(f, area, &text, style));
    let x = text_x(app);
    let gap = if used > 0 { used + 2 } else { 1 };
    let room = area.width.saturating_sub(x + gap);
    let done = s.state == SavedState::Completed;
    let mut spans = Vec::new();
    let mut left = usize::from(room);
    if done {
        spans.push(Span::styled("✓ ", t.muted()));
        left = left.saturating_sub(2);
    }
    let text = clip(s.text.line().as_str(), left);
    spans.push(Span::styled(text, if done { t.muted() } else { t.text() }));
    put(f, area, x, room, spans);
}

fn meta(f: &mut Frame, app: &App, i: usize, area: Rect) {
    let t = &app.theme;
    let s = app.model.saved(i);
    let author = app.model.user(&s.author).map(|u| u.display_name.line().into_string()).unwrap_or_default();
    let target = Target::Conversation { workspace: s.workspace.clone(), conversation: s.conversation.clone() };
    let place = app.model.target(&target).map(breadcrumb).unwrap_or_default();
    let date = crate::time::date(s.ts);
    // The year only when it is not this one.
    let date = match date.split_once('-') {
        Some((y, md)) if crate::time::date(app.model.taken()).starts_with(y) => md.to_string(),
        _ => date,
    };
    // Short of room the author goes first (the chip names them), then the place is cut: never
    // the date.
    let date = format!(" · {date}");
    let x = text_x(app);
    let room = area.width.saturating_sub(x + 1);
    let left = usize::from(room).saturating_sub(width(&date));
    let full = app.i18n.msg(&Msg::TodoMeta { author, place: place.clone() }).to_string();
    let main = if width(&full) <= left { full } else { clip(&place, left) };
    put(f, area, x, room, vec![Span::styled(main, t.muted()), Span::styled(date, t.faint())]);
}
