//! A pane's messages, laid out only as far as the screen reaches: from the message at the bottom
//! upwards until the area is full. A message's rows are its date separator (on a new day), its
//! text wrapped beside the sender's name with the time on the right, a cut marker for very long
//! text, the "N replies" row of a thread and its reaction pills.
//!
//! ```text
//! ──────────────── 2026-01-05 ────────────────
//! Kim           Starting deploy          10:02
//!               ⤷ 4 replies · last 10:15
//! Park          PR is up (edited)        10:20
//!               :eyes: 2  :+1: 1
//! ```
//!
//! Every row laid out is counted ([`rows_laid_out`]): the performance budget holds a frame to
//! at most twice the rows the area shows, however long the history.

use super::highlight;
use crate::app::App;
use crate::app::pane::Pane;
use crate::text::{clip, width, wrap};
use crate::time;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::i18n::{Label, Msg};
use std::cell::Cell;
use std::collections::HashMap;

/// The sender's column, at most.
const AUTHOR_WIDTH: usize = 14;
/// The time column (`10:02` and a space before it).
const TIME_WIDTH: usize = 6;
/// Rows of text a message shows at most; the rest is marked, not drawn.
pub const MAX_TEXT_ROWS: usize = 40;

thread_local! {
    static LAID_OUT: Cell<u64> = const { Cell::new(0) };
}

/// Rows laid out on this thread since the last [`reset_rows_laid_out`] (for the budget tests).
pub fn rows_laid_out() -> u64 {
    LAID_OUT.with(Cell::get)
}

pub fn reset_rows_laid_out() {
    LAID_OUT.with(|c| c.set(0));
}

/// The rows of message `i` of `pane` for a width of `w` cells.
fn rows(app: &App, pane: &Pane, i: usize, w: usize) -> Vec<Line<'static>> {
    let t = &app.theme;
    let m = &pane.items[i];
    let mut out: Vec<Line<'static>> = Vec::new();
    let new_day = i == 0 || time::day(pane.items[i - 1].ts) != time::day(m.ts);
    if new_day {
        let label = format!(" {} ", time::date(m.ts));
        let side = w.saturating_sub(width(&label)) / 2;
        let rule = "─".repeat(side);
        out.push(Line::styled(format!("{rule}{label}{rule}"), t.timestamp));
    }
    let author_w = AUTHOR_WIDTH.min(w / 4).max(4);
    let text_w = w.saturating_sub(author_w + TIME_WIDTH).max(4);
    let indent = " ".repeat(author_w);
    let (mut lines, more) = wrap(m.text.as_str(), text_w, MAX_TEXT_ROWS);
    let edited = app.i18n.label(Label::MessageEdited).to_string();
    // The edited marker goes after the last line of text when it fits there.
    let mut edited_row = false;
    if m.edited {
        let last = lines.last_mut().expect("wrap gives a line");
        if width(last) + 1 + width(&edited) <= text_w {
            last.push(' ');
        } else {
            edited_row = true;
        }
    }
    let n = lines.len();
    for (k, l) in lines.into_iter().enumerate() {
        let mut spans: Vec<Span<'static>> = Vec::new();
        if k == 0 {
            let name = clip(m.author.as_str(), author_w - 1);
            let pad = " ".repeat(author_w - width(&name));
            let style = if m.own { t.own_author } else { t.author };
            spans.push(Span::styled(format!("{name}{pad}"), style));
        } else {
            spans.push(Span::raw(indent.clone()));
        }
        let lw = width(&l);
        spans.push(Span::styled(l, t.text));
        let mut used = lw;
        if m.edited && !edited_row && k + 1 == n {
            used += width(&edited);
            spans.push(Span::styled(edited.clone(), t.muted));
        }
        if k == 0 {
            spans.push(Span::raw(" ".repeat(text_w.saturating_sub(used) + 1)));
            spans.push(Span::styled(time::hm(m.ts), t.timestamp));
        }
        out.push(Line::from(spans));
    }
    if edited_row {
        out.push(Line::from(vec![Span::raw(indent.clone()), Span::styled(edited, t.muted)]));
    }
    if more {
        let label = app.i18n.label(Label::MessageMore).to_string();
        out.push(Line::from(vec![Span::raw(indent.clone()), Span::styled(label, t.muted)]));
    }
    if let Some(th) = m.thread.filter(|_| !pane.is_thread()) {
        let msg = Msg::MessageReplies { count: u64::from(th.replies), time: time::hm(th.last_reply) };
        let text = format!("⤷ {}", app.i18n.msg(&msg));
        out.push(Line::from(vec![Span::raw(indent.clone()), Span::styled(text, t.thread_link)]));
    }
    if !m.reactions.is_empty() {
        let mut spans = vec![Span::raw(indent)];
        for (k, r) in m.reactions.iter().enumerate() {
            if k > 0 {
                spans.push(Span::raw("  "));
            }
            let style = if r.mine { t.reaction_mine } else { t.reaction };
            spans.push(Span::styled(format!(":{}: {}", r.name, r.count), style));
        }
        out.push(Line::from(spans));
    }
    LAID_OUT.with(|c| c.set(c.get() + out.len() as u64));
    out
}

/// Draw `pane`'s messages into `area`; `focused`: the pane has the keyboard (the selection is
/// drawn as the cursor, else underlined).
pub(super) fn draw(f: &mut Frame, app: &App, pane: &Pane, area: Rect, focused: bool) {
    let t = &app.theme;
    if area.height == 0 || area.width == 0 {
        return;
    }
    if pane.items.is_empty() {
        let label = if pane.complete { Label::PaneNoMessages } else { Label::PaneLoading };
        f.render_widget(Paragraph::new(app.i18n.label(label).to_string()).style(t.muted), area);
        return;
    }
    let (h, w) = (usize::from(area.height), usize::from(area.width));
    let n = pane.items.len();
    let mut laid: HashMap<usize, Vec<Line<'static>>> = HashMap::new();
    let height = |i: usize, laid: &mut HashMap<usize, Vec<Line<'static>>>| {
        laid.entry(i).or_insert_with(|| rows(app, pane, i, w)).len()
    };
    // Keep the selection on screen: below the view, it becomes the bottom; above it, the top.
    let mut bottom = pane.bottom.get().unwrap_or(n - 1).min(n - 1);
    if let Some(sel) = pane.selected.map(|s| s.min(n - 1)) {
        if sel > bottom {
            bottom = sel;
        } else {
            // Every message takes a row at least: more messages than rows never fit, and a
            // jump (`gg`) lays out only the rows it shows.
            let fits = bottom - sel < h && {
                let mut total = 0;
                let mut i = bottom;
                loop {
                    total += height(i, &mut laid);
                    if i == sel {
                        break total <= h;
                    }
                    if total > h {
                        break false;
                    }
                    i -= 1;
                }
            };
            if !fits {
                let mut total = 0;
                bottom = sel;
                for j in sel..n {
                    let hj = height(j, &mut laid);
                    if j > sel && total + hj > h {
                        break;
                    }
                    total += hj;
                    bottom = j;
                }
            }
        }
    }
    // At the newest, new messages stay in view; elsewhere the view stays put.
    pane.bottom.set((bottom != n - 1).then_some(bottom));
    // From the bottom message upwards until the area is full.
    let mut shown: Vec<(usize, Vec<Line<'static>>)> = Vec::new();
    let mut total = 0;
    let mut i = bottom;
    loop {
        let rows = laid.remove(&i).unwrap_or_else(|| rows(app, pane, i, w));
        total += rows.len();
        shown.push((i, rows));
        if total >= h || i == 0 {
            break;
        }
        i -= 1;
    }
    shown.reverse();
    let range = pane.range();
    let style = if focused { t.cursor } else { t.cursor_inactive };
    // Rows cut off at the top when the area is full; rows start at the bottom otherwise.
    let mut skip = total.saturating_sub(h);
    let mut y = area.y + h.saturating_sub(total) as u16;
    for (i, rows) in shown {
        let selected = range.is_some_and(|(a, b)| (a..=b).contains(&i));
        for line in rows {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let row = Rect { y, height: 1, ..area };
            f.render_widget(Paragraph::new(line), row);
            if selected {
                highlight(f, row, style);
            }
            y += 1;
        }
    }
}
