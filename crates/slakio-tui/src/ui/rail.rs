//! The rail: workspaces (color stripe, letter or name, unread mark), a separator, then Home,
//! DMs, Activity, Files and Later. Collapsed it shows letters (or icons); focused or hovered it
//! expands with names and counts, over the list panel (which stays drawn beside it) or pushing
//! it aside. Only the keyboard's focus gives it the accent border and a cursor.
//!
//! ```text
//! collapsed   expanded
//! ╭──╮        ╭───────────────────╮
//! │▌A│        │▌A company      3  │   3 = a pill of mentions; ● = unread
//! │●B│        │▌B side         ●  │   collapsed, ● in the stripe's place = unread
//! │──│        │───────────────────│
//! │H │        │▎H  Home           │   ▎ and the accent = what the list panel shows
//! │D●│        │ D  DMs       16   │
//! ```

use super::{count_pill, frame, highlight, view_glyph, view_label, workspace_letter};
use crate::app::App;
use crate::app::shell::{RailItem, Region, View, rail_items};
use crate::screen;
use crate::text::width;
use crate::theme::{GUTTER, Selection};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let expanded = app.shell.rail_expanded();
    let focused = app.shell.focus == Region::Rail;
    // Over the list panel only its own cells are cleared: the list shows beside it.
    f.render_widget(Clear, area);
    f.buffer_mut().set_style(area, t.base());
    f.render_widget(frame(app, Region::Rail, ""), area);
    let inner = screen::inner(area);
    let workspaces = app.model.workspaces();
    let items = rail_items(workspaces.len());
    let mut y = inner.y;
    for (i, item) in items.iter().enumerate() {
        if y >= inner.bottom() {
            break;
        }
        if i == workspaces.len() {
            let sep = "─".repeat(usize::from(inner.width));
            f.render_widget(Paragraph::new(sep).style(t.divider()), Rect { y, height: 1, ..inner });
            y += 1;
            if y >= inner.bottom() {
                break;
            }
        }
        let row = Rect { y, height: 1, ..inner };
        let (left, mark, mark_style) = match *item {
            RailItem::Workspace(w) => {
                let ws = &workspaces[w];
                let style = if w == app.shell.workspace { t.current() } else { t.text() };
                let mentions = app.model.workspace_mentions(w);
                let unread = app.model.workspace_unread(w);
                let name = ws.name.line();
                if expanded {
                    let left = vec![Span::styled("▌", t.workspace(ws.color)), Span::styled(name.into_string(), style)];
                    match (mentions, unread) {
                        (m, _) if m > 0 => (left, count_pill(m), t.badge()),
                        (_, true) => (left, "●".to_string(), t.dot(false)),
                        _ => (left, String::new(), style),
                    }
                } else {
                    // Collapsed there is no room for a dot: an unread workspace's stripe is one.
                    let stripe = if unread || mentions > 0 { "●" } else { "▌" };
                    let left =
                        vec![Span::styled(stripe, t.workspace(ws.color)), Span::styled(workspace_letter(&name), style)];
                    (left, String::new(), style)
                }
            }
            RailItem::View(v) => {
                let current = v == app.shell.view;
                let style = if current { t.current() } else { t.text() };
                let glyph = Span::styled(view_glyph(v, app.settings.icons), style);
                let count = match v {
                    View::Dms => app.model.dm_unread(app.shell.workspace),
                    View::Activity => app.model.mentions(),
                    _ => 0,
                };
                if expanded {
                    let marker = Span::styled(if current { GUTTER } else { " " }, t.current());
                    let label = Span::styled(format!("  {}", app.i18n.label(view_label(v))), style);
                    let mark = if count > 0 { count_pill(count) } else { String::new() };
                    (vec![marker, glyph, label], mark, t.badge())
                } else {
                    let dot = if count > 0 { "●" } else { "" };
                    (vec![glyph], dot.to_string(), t.dot(v == View::Activity))
                }
            }
        };
        f.render_widget(Paragraph::new(Line::from(left)), row);
        draw_mark(f, row, &mark, mark_style, expanded);
        // The cursor leaves the workspace's stripe its color.
        let bar = match item {
            RailItem::Workspace(_) => Rect { x: row.x + 1, width: row.width.saturating_sub(1), ..row },
            RailItem::View(_) => row,
        };
        if focused && i == app.shell.rail_cursor {
            highlight(f, app, bar, Selection::Focused);
        } else if !focused && expanded && app.shell.hover_item == Some(i) {
            highlight(f, app, bar, Selection::Unfocused);
        }
        y += 1;
    }
}

/// A count pill or dot at the right of `row` (one cell in from the edge when expanded; the last
/// cell when collapsed).
fn draw_mark(f: &mut Frame, row: Rect, mark: &str, style: Style, expanded: bool) {
    if mark.is_empty() {
        return;
    }
    let w = width(mark) as u16;
    let pad = u16::from(expanded);
    let x = row.right().saturating_sub(w + pad).max(row.x);
    let at = Rect { x, width: w.min(row.width), ..row };
    f.render_widget(Paragraph::new(mark.to_string()).style(style), at);
}
