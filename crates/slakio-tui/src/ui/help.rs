//! The keyboard help over the dimmed screen: a search line, then every context's keys (the one
//! the keyboard was in open first), each row the action's label and its keys on the right.
//!
//! ```text
//! ╭ Keys — List panel ─────────────────────────────────────────╮
//! │ / Type / to search                                          │
//! │─────────────────────────────────────────────────────────────│
//! │ ▾ List panel                                                │
//! │   Next row                                         j / Down │
//! │ ▸ Rail                                                    8 │
//! ╰──────────────────── ↑↓ move · Enter run · / search · Esc close ╯
//! ```

use super::modal;
use crate::app::App;
use crate::app::help::Row;
use crate::text::{clip, width};
use crate::theme::Selection;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::i18n::{Label, Msg};

/// The widest the help box gets.
const MAX_W: u16 = 100;

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let Some(h) = app.help.as_ref() else { return };
    let rows = h.rows(&app.keymap, &app.i18n);
    let w = area.width.saturating_sub(4).min(MAX_W);
    let hh = area.height.saturating_sub(2);
    if w < 20 || hh < 6 {
        return;
    }
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - hh) / 2, w, hh);
    let context = app.i18n.label(h.origin.label()).to_string();
    let title = app.i18n.msg(&Msg::HelpTitle { context }).to_string();
    let inner = modal(f, app, rect, &title, &app.i18n.label(Label::HelpKeys));
    let iw = usize::from(inner.width);
    // The search line, then a divider.
    let search = if h.filter.is_empty() && !h.typing {
        Line::from(vec![
            Span::styled(" / ", t.key()),
            Span::styled(app.i18n.label(Label::HelpSearch).to_string(), t.muted()),
        ])
    } else {
        Line::from(vec![Span::styled(" / ", t.key()), Span::styled(h.filter.clone(), t.text())])
    };
    f.render_widget(Paragraph::new(search), Rect { height: 1, ..inner });
    if h.typing {
        let x = inner.x + 3 + width(&h.filter) as u16;
        f.set_cursor_position(Position { x: x.min(inner.right().saturating_sub(1)), y: inner.y });
    }
    let rule = "─".repeat(iw);
    f.render_widget(Paragraph::new(rule).style(t.divider()), Rect { y: inner.y + 1, height: 1, ..inner });
    let list = Rect { y: inner.y + 2, height: inner.height.saturating_sub(2), ..inner };
    let view = usize::from(list.height).max(1);
    h.height.set(view);
    if rows.is_empty() {
        let none = app.i18n.msg(&Msg::HelpNone { query: h.filter.clone() }).to_string();
        f.render_widget(
            Paragraph::new(clip(&none, iw.saturating_sub(2))).style(t.faint()),
            Rect { x: list.x + 1, height: 1, ..list },
        );
        return;
    }
    let cursor = h.cursor.min(rows.len() - 1);
    // Keep the cursor's row in view.
    let mut top = h.top.get().min(rows.len().saturating_sub(1));
    if cursor < top {
        top = cursor;
    } else if cursor >= top + view {
        top = cursor + 1 - view;
    }
    h.top.set(top);
    let keys_w = rows
        .iter()
        .filter_map(|r| if let Row::Entry { keys, .. } = r { Some(width(keys)) } else { None })
        .max()
        .unwrap_or(0)
        .min(iw / 2);
    for (i, row) in rows.iter().enumerate().skip(top).take(view) {
        let y = list.y + (i - top) as u16;
        let r = Rect { y, height: 1, ..list };
        match row {
            Row::Section { ctx, open, count } => {
                let mark = if *open { "▾" } else { "▸" };
                let name = app.i18n.label(ctx.label()).to_string();
                let n = if *open { String::new() } else { count.to_string() };
                let room = iw.saturating_sub(4 + width(&n) + 1);
                let line = Line::from(vec![
                    Span::styled(format!(" {mark} "), t.faint()),
                    Span::styled(clip(&name, room), t.current()),
                ]);
                f.render_widget(Paragraph::new(line), r);
                if !n.is_empty() {
                    let x = r.right().saturating_sub(width(&n) as u16 + 1);
                    f.render_widget(Paragraph::new(n).style(t.faint()), Rect { x, width: r.right() - x, ..r });
                }
            }
            Row::Entry { label, keys, .. } => {
                let label_w = iw.saturating_sub(keys_w + 6);
                let text = clip(&app.i18n.label(*label), label_w);
                f.render_widget(
                    Paragraph::new(Span::styled(text, t.text())),
                    Rect { x: r.x + 3, width: label_w as u16, ..r },
                );
                // Keys flush right.
                let k = clip(keys, keys_w);
                let kw = width(&k) as u16;
                let x = r.right().saturating_sub(kw + 1);
                f.render_widget(Paragraph::new(Span::styled(k, t.key())), Rect { x, width: kw, ..r });
            }
        }
        if i == cursor {
            super::highlight(f, app, r, Selection::Focused);
        }
    }
}
