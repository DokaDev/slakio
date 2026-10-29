//! A pane's messages, laid out only as far as the screen reaches: from the message at the bottom
//! upwards until the area is full. Each message's rows come from [`message`] (the comfortable
//! layout, or the compact one): the blank and the date separator above it, its header and text,
//! the "N replies" link, its reactions as chips.
//!
//! A conversation sits at the bottom, by the composer, as in any chat; a thread starts at the
//! top: its message, a `── N replies ──` divider, then the replies (the newest stay in view once
//! they fill the pane). The selection paints every row of its message's block (never the blank
//! between blocks or a date separator).
//!
//! Every row laid out is counted ([`rows_laid_out`]): the performance budget holds a frame to
//! at most twice the rows the area shows, however long the history.

pub mod body;
pub mod message;

use super::highlight;
use crate::action::{Action, PaneAction};
use crate::app::App;
use crate::app::pane::{Hit, Pane};
use crate::keymap::Ctx;
use crate::theme::Selection;
use message::{Kind, Laid};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;
use slakio_core::i18n::{Label, Msg};
use std::cell::Cell;
use std::collections::HashMap;

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

/// Draw `pane`'s messages into `area`; `focused`: the pane has the keyboard (the selection is
/// drawn as the bar of the focused panel, else as the faint one).
#[expect(clippy::too_many_lines, reason = "anchoring and painting in one pass; to be split")]
pub(super) fn draw(f: &mut Frame, app: &App, pane: &Pane, area: Rect, focused: bool) {
    let tl = app.work.timeline(pane);
    let t = &app.theme;
    pane.hits.borrow_mut().clear();
    if area.height == 0 || area.width < 3 {
        return;
    }
    // One cell of padding on each side; the left one is the selection's gutter.
    let text_area = |row: Rect| Rect { x: row.x + 1, width: row.width.saturating_sub(2), ..row };
    if tl.items.is_empty() {
        let text = if tl.complete && app.read_only() {
            ratatui::text::Text::styled(app.i18n.label(Label::PaneNoMessagesReadOnly).to_string(), t.faint())
        } else if tl.complete {
            let keys = super::empty::key_of(app, Action::Pane(PaneAction::Insert), Ctx::PaneNormal).unwrap_or_default();
            ratatui::text::Text::styled(app.i18n.msg(&Msg::PaneNoMessages { keys }).to_string(), t.faint())
        } else if tl.failed.is_some() {
            super::failed_text(app, Label::PaneFailed, tl.retryable())
        } else {
            ratatui::text::Text::styled(app.i18n.label(Label::PaneLoading).to_string(), t.faint())
        };
        f.render_widget(Paragraph::new(text), text_area(area));
        return;
    }
    let (h, w) = (usize::from(area.height), usize::from(area.width.saturating_sub(2)).max(1));
    let n = tl.items.len();
    let mut laid: HashMap<usize, Vec<Laid>> = HashMap::new();
    let selected = pane.selected_index(tl);
    let range = pane.range(tl);
    let selected_one = selected;
    let rows = |i: usize| {
        let out = message::rows(app, pane, tl, i, w, selected == Some(i) && range.is_none_or(|(a, b)| a == b));
        LAID_OUT.with(|c| c.set(c.get() + out.len() as u64));
        out
    };
    let height = |i: usize, laid: &mut HashMap<usize, Vec<Laid>>| laid.entry(i).or_insert_with(|| rows(i)).len();
    // A selection held on its row (messages came in between it and the bottom) stays there:
    // the bottom is the message that many rows below it, its last rows cut off when it does
    // not end there.
    let held = pane.held.get().filter(|(ts, below)| pane.selected == Some(*ts) && *below < h);
    pane.held.set(held);
    let mut cut = 0;
    // Keep the selection on screen: below the view, it becomes the bottom; above it, the top.
    let mut bottom = pane.bottom.get().and_then(|ts| tl.index_near(ts)).unwrap_or(n - 1).min(n - 1);
    if let (Some((_, below)), Some(sel)) = (held, selected.map(|s| s.min(n - 1))) {
        let (mut total, mut j) = (0, sel);
        while total < below && j + 1 < n {
            j += 1;
            total += height(j, &mut laid);
        }
        (bottom, cut) = (j, total.saturating_sub(below));
    } else if let Some(sel) = selected.map(|s| s.min(n - 1)) {
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
    // At the newest, new messages stay in view (none is new below it any more); elsewhere the
    // view stays put.
    pane.bottom.set((bottom != n - 1).then(|| tl.items[bottom].ts));
    if bottom == n - 1 && cut == 0 {
        pane.fresh.set(None);
        pane.seen.borrow_mut().clear();
    }
    // From the bottom message upwards until the area is full (and the rows cut off below).
    let reach = h + cut;
    let mut shown: Vec<(usize, Vec<Laid>)> = Vec::new();
    let mut total = 0;
    let mut i = bottom;
    loop {
        let rows = laid.remove(&i).unwrap_or_else(|| rows(i));
        total += rows.len();
        shown.push((i, rows));
        if total >= reach || i == 0 {
            break;
        }
        i -= 1;
    }
    shown.reverse();
    // The oldest is on screen with rows to spare and newer messages below the bottom one (after
    // `gg`): those fill the rest, so the history starts on the first row with no gap above it.
    // A conversation that fits whole stays at the bottom, as in any chat; a thread starts at
    // the top.
    let from_top = cut == 0 && total < h && (bottom + 1 < n || pane.is_thread());
    let mut i = bottom + 1;
    while cut == 0 && total < h && i < n {
        let rows = laid.remove(&i).unwrap_or_else(|| rows(i));
        total += rows.len();
        shown.push((i, rows));
        i += 1;
    }
    let how = |i: usize| match (focused, pane.visual.is_some()) {
        (_, true) if selected != Some(i) => Selection::Visual,
        (true, _) => Selection::Focused,
        (false, _) => Selection::Unfocused,
    };
    // Rows cut off at the top when the area is full (at the bottom when filled from the top);
    // rows start at the bottom otherwise.
    let mut skip = if from_top { 0 } else { total.saturating_sub(reach) };
    let mut y = if from_top { area.y } else { area.y + h.saturating_sub(total) as u16 };
    let mut hits = pane.hits.borrow_mut();
    let mut sel_last = None;
    for (i, rows) in shown {
        let selected = range.is_some_and(|(a, b)| (a..=b).contains(&i));
        for (line, kind) in rows {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if y >= area.bottom() {
                break;
            }
            if Some(i) == selected_one {
                sel_last = Some(y);
            }
            let row = Rect { y, height: 1, ..area };
            f.render_widget(Paragraph::new(line), text_area(row));
            if selected && matches!(kind, Kind::Content | Kind::Link) {
                highlight(f, app, row, how(i));
            }
            if kind != Kind::Gap {
                hits.push(Hit { y, message: i, link: kind == Kind::Link });
            }
            y += 1;
        }
    }
    // Where the selection is, for a change that must not move it ([`Pane::held`]).
    let below = sel_last.map(|y| usize::from(area.bottom() - 1 - y));
    pane.below.set(pane.selected.zip(below));
}
