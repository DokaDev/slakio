//! The which-key popup: after the first key of a sequence (`Space`, `Space w`, `g`) and a short
//! wait ([`crate::app::WHICH_KEY_DELAY`]), a box above the status line lists the keys that may
//! follow, column by column: the key, then what it does (a group of more keys as `+Name`).
//!
//! ```text
//! ╭ Space — Leader ──────────────────────────────────────────────────────────╮
//! │ h  Show Home            w  +Window              [  Back to the …         │
//! │ d  Show DMs             e  Show or hide the …   ]  Forward again         │
//! ╰──────────────────────────────────── Esc close · Backspace back · ? all keys ╯
//! ```

use super::modal;
use crate::app::App;
use crate::keymap::{guide, keys};
use crate::text::{clip, width};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::i18n::{Label, Msg};

/// The widest column (longer labels end in `…`).
const COLUMN: usize = 40;

/// Draw the popup along the bottom of `area` (the screen above the status line).
pub(super) fn draw(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let pending = app.keys.pending();
    let Some(ctx) = app.keys.ctx() else { return };
    let next = guide::next(&app.keymap, ctx, pending);
    if next.is_empty() || area.height < 4 {
        return;
    }
    let items: Vec<(String, String, bool)> = next
        .iter()
        .map(|n| {
            let label = app.i18n.label(guide::label(n)).to_string();
            let group = matches!(n.what, guide::What::Group(_));
            (n.key.clone(), if group { format!("+{label}") } else { label }, group)
        })
        .collect();
    let key_w = items.iter().map(|(k, _, _)| width(k)).max().unwrap_or(1);
    let col_w = (items.iter().map(|(_, l, _)| key_w + 2 + width(l)).max().unwrap_or(1)).min(COLUMN) + 3;
    let inner_w = usize::from(area.width.saturating_sub(2));
    let cols = (inner_w / col_w).clamp(1, items.len());
    let max_rows = usize::from(area.height / 2).max(1);
    let rows = items.len().div_ceil(cols).clamp(1, max_rows);
    let h = rows as u16 + 2;
    let rect = Rect::new(area.x, area.bottom().saturating_sub(h), area.width, h.min(area.height));
    let group = guide::group(pending).map(|l| app.i18n.label(l).to_string()).unwrap_or_default();
    let title = app.i18n.msg(&Msg::GuideTitle { keys: keys::label(pending), group }).to_string();
    let inner = modal(f, app, rect, &title, &app.i18n.label(Label::GuideFooter));
    let cell_w = usize::from(inner.width) / cols;
    let fits = rows * cols;
    for (i, (k, l, group)) in items.iter().enumerate().take(fits) {
        let (col, row) = (i / rows, i % rows);
        let x = inner.x + (col * cell_w) as u16 + 1;
        let y = inner.y + row as u16;
        let room = cell_w.saturating_sub(2);
        let text = if i + 1 == fits && items.len() > fits {
            Line::styled("…", t.muted())
        } else {
            let pad = " ".repeat(key_w.saturating_sub(width(k)) + 2);
            let label = clip(l, room.saturating_sub(key_w + 2));
            Line::from(vec![
                Span::styled(k.clone(), t.key()),
                Span::raw(pad),
                Span::styled(label, if *group { t.warm() } else { t.text() }),
            ])
        };
        f.render_widget(Paragraph::new(text), Rect::new(x, y, room as u16, 1).intersection(inner));
    }
}
