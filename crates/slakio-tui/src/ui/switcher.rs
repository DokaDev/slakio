//! The workspace switcher: a small popup under the top bar's workspace chip, one row per
//! workspace — its color band, its name and what it holds unread (`@3`, `●`) — the cursor on
//! the one to switch to.
//!
//! ```text
//! ╭ Workspaces ──────────────────╮
//! │ ▌A company                   │
//! │ ▌B side                 @2   │
//! ╰──── Enter switch · Esc close ╯
//! ```

use super::modal;
use crate::app::App;
use crate::app::Unread;
use crate::text::{clip, width};
use crate::theme::Selection;
use ratatui::Frame;
use ratatui::layout::Rect;
use slakio_core::i18n::Label;

pub(super) fn draw(f: &mut Frame, app: &App) {
    let (Some(rect), Some(cursor)) = (app.switcher_box(), app.switcher()) else { return };
    let t = &app.theme;
    let title = app.i18n.label(Label::SwitcherTitle).to_string();
    let footer = app.i18n.label(Label::SwitcherKeys).to_string();
    let inner = modal(f, app, rect, &title, &footer);
    let first = crate::screen::switcher_first(cursor, usize::from(inner.height));
    for (i, w) in app.model.workspaces().iter().enumerate().skip(first) {
        let y = inner.y + (i - first) as u16;
        if y >= inner.bottom() {
            break;
        }
        let row = Rect { y, height: 1, ..inner };
        let unread = Unread {
            mentions: app.model.workspace_mentions(i),
            any: app.model.workspace_unread(i),
            ..Unread::default()
        };
        let mark = unread.badge();
        let mark_w = mark.as_deref().map_or(0, width);
        let buf = f.buffer_mut();
        buf.set_string(row.x + 1, y, "▌", t.workspace(w.color));
        let room = usize::from(row.width).saturating_sub(4 + mark_w);
        let name_style = if i == app.shell.workspace { t.current() } else { t.text() };
        buf.set_string(row.x + 2, y, clip(w.name.line().as_str(), room), name_style);
        if let Some(m) = &mark {
            let x = row.right().saturating_sub(mark_w as u16 + 1);
            buf.set_string(x, y, m, t.marker(m.starts_with('@')));
        }
        if i == cursor {
            let bar = Rect { x: row.x + 2, width: row.width.saturating_sub(2), ..row };
            t.paint_selection(f.buffer_mut(), bar, Selection::Focused);
        }
    }
}
