//! The rail: workspaces (colour stripe, letter, unread dot), a separator, then Home, DMs,
//! Activity, Files and Later. Collapsed it shows letters (or icons); focused or hovered it
//! expands with names and counts, over the list panel or pushing it aside.
//!
//! ```text
//! collapsed   expanded
//! ╭──╮        ╭───────────────────╮
//! │▌A│        │▌A A company      ●│
//! │▌B│        │▌B B side          │
//! │──│        │───────────────────│
//! │H │        │H  Home            │
//! │D●│        │D  DMs            2│
//! ```

use super::{frame, highlight, view_glyph, view_label, workspace_letter};
use crate::app::App;
use crate::app::shell::{RailItem, Region, View, rail_items};
use crate::screen;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let expanded = app.shell.rail_expanded();
    f.render_widget(Clear, area);
    f.render_widget(frame(app, Region::Rail, Line::default()), area);
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
            f.render_widget(Paragraph::new(sep).style(t.border), Rect { y, height: 1, ..inner });
            y += 1;
            if y >= inner.bottom() {
                break;
            }
        }
        let row = Rect { y, height: 1, ..inner };
        let (left, right) = match *item {
            RailItem::Workspace(w) => {
                let ws = &workspaces[w];
                let style = if w == app.shell.workspace { t.current } else { t.text };
                // Collapsed, the name's first letter; expanded, the name.
                let name = if expanded { ws.name.clone() } else { workspace_letter(&ws.name) };
                let spans = vec![Span::styled("▌", t.workspace(ws.color)), Span::styled(name, style)];
                // Collapsed, the letter takes the whole row.
                let dot = if expanded && app.model.workspace_unread(w) { "●" } else { "" };
                (spans, dot.to_string())
            }
            RailItem::View(v) => {
                let style = if v == app.shell.view { t.current } else { t.text };
                let mut spans = vec![Span::styled(view_glyph(v, app.settings.icons), style)];
                if expanded {
                    spans.push(Span::styled(format!("  {}", app.i18n.label(view_label(v))), style));
                }
                let count = match v {
                    View::Dms => app.model.dm_unread(app.shell.workspace),
                    View::Activity => app.model.mentions(),
                    _ => 0,
                };
                let badge = match (count, expanded) {
                    (0, _) => String::new(),
                    (n, true) => n.to_string(),
                    (_, false) => "●".to_string(),
                };
                (spans, badge)
            }
        };
        f.render_widget(Paragraph::new(Line::from(left)), row);
        if !right.is_empty() {
            let w = Span::raw(right.as_str()).width() as u16;
            let x = row.right().saturating_sub(w);
            let at = Rect { x: x.max(row.x), width: w.min(row.width), ..row };
            let style = if matches!(item, RailItem::View(View::Activity)) { t.mention } else { t.unread };
            f.render_widget(Paragraph::new(right).style(style), at);
        }
        if app.shell.focus == Region::Rail && i == app.shell.rail_cursor {
            highlight(f, row, t.cursor);
        }
        y += 1;
    }
}
