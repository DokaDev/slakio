//! The workspace switcher: a small popup under the list panel's workspace chip, one row per
//! workspace — its color band, its name and what it holds unread (`@3`, `●`), or that it could
//! not be loaded or Slack signed it out (`signed out`), and faintly that its live updates
//! stopped (`live off`) — the cursor on the one to switch to.
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
use slakio_core::backend::FailKind;
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
            mentions: app.model.workspace_mentions(&w.id),
            any: app.model.workspace_unread(&w.id),
            ..Unread::default()
        };
        // A workspace that could not be loaded says so, quietly, where its count would be.
        // So does one Slack signed out while the app ran, which still shows what it loaded.
        let failed = app.boot_failed_in(&w.id).filter(|_| !app.model.is_loaded_in(&w.id));
        let failed =
            failed.map(|k| if *k == FailKind::Auth { Label::SwitcherSignedOut } else { Label::SwitcherFailed });
        let failed = failed.or(app.signed_out_in(&w.id).then_some(Label::SwitcherSignedOut));
        let failed = failed.map(|l| app.i18n.label(l).to_string());
        let mark = failed.clone().or_else(|| unread.badge());
        let mark_w = mark.as_deref().map_or(0, width);
        // Its live updates stopped: said faintly before its count, until they are back.
        let off = app.model.live_off(&w.id).filter(|_| failed.is_none());
        let off = off.map(|_| app.i18n.label(Label::StatusLiveOff).to_string());
        let off_w = off.as_deref().map_or(0, |o| width(o) + 1);
        let buf = f.buffer_mut();
        buf.set_string(row.x + 1, y, "▌", t.workspace(w.color));
        let room = usize::from(row.width).saturating_sub(4 + mark_w + off_w);
        let name_style = if i == app.workspace() { t.current() } else { t.text() };
        buf.set_string(row.x + 2, y, clip(w.name.line().as_str(), room), name_style);
        if let Some(m) = &mark {
            let x = row.right().saturating_sub(mark_w as u16 + 1);
            let style = if failed.is_some() { t.muted() } else { t.marker(m.starts_with('@')) };
            buf.set_string(x, y, m, style);
        }
        if let Some(o) = &off {
            let x = row.right().saturating_sub((mark_w + off_w) as u16 + u16::from(mark.is_some()));
            buf.set_string(x, y, o, t.faint());
        }
        if i == cursor {
            let bar = Rect { x: row.x + 2, width: row.width.saturating_sub(2), ..row };
            t.paint_selection(f.buffer_mut(), bar, Selection::Focused);
        }
    }
}
