//! A pane's messages, laid out only as far as the screen reaches: from the message at the bottom
//! upwards until the area is full. A message's rows are its date separator (on a new day), its
//! text wrapped beside the sender's name with the time on the right, a cut marker for very long
//! text, the "N replies" row of a thread and its reaction pills. A message right after one of
//! the same sender (within five minutes, the same day) leaves the name out. In a pane narrower
//! than [`STACKED_BELOW`] the name and time head the message and the text goes below them.
//!
//! ```text
//! ──────────────── 2026-01-05 ────────────────     Kim · 10:02
//! Kim           Starting deploy          10:02       Starting deploy
//!               ⤷ 4 replies · last 10:15             ⤷ 4 replies · last 10:15
//!               Rolled back              10:04       Rolled back
//! Park          PR is up (edited)        10:20     Park · 10:20
//!               :eyes: 2  :+1: 1                     PR is up (edited)
//! ```
//!
//! A conversation sits at the bottom, by the composer, as in any chat; a thread starts at the
//! top: its message, a `── N replies ──` divider, then the replies (the newest stay in view once
//! they fill the pane).
//!
//! Every row laid out is counted ([`rows_laid_out`]): the performance budget holds a frame to
//! at most twice the rows the area shows, however long the history.

use super::highlight;
use crate::action::{Action, PaneAction};
use crate::app::App;
use crate::app::pane::{Hit, Pane};
use crate::keymap::Ctx;
use crate::text::{clip, width, wrap};
use crate::theme::Selection;
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
/// A pane whose inside is narrower than this stacks each message: name and time, then the text.
pub const STACKED_BELOW: usize = 56;
/// Messages of one sender this close together form a group: the name shows once.
const GROUP_SECS: u64 = 5 * 60;

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

/// A row laid out, and whether it is the message's "N replies" link.
type Laid = (Line<'static>, bool);

/// Message `i` follows one of the same sender closely: its name is left out.
fn grouped(pane: &Pane, i: usize) -> bool {
    let Some(prev) = i.checked_sub(1).map(|p| &pane.items[p]) else { return false };
    let m = &pane.items[i];
    // A thread's own message and its first reply never group: the divider is between them.
    let root = pane.root();
    prev.user == m.user
        && time::day(prev.ts) == time::day(m.ts)
        && m.ts.0.saturating_sub(prev.ts.0) <= GROUP_SECS * 1_000_000
        && root != Some(i - 1)
        && prev.thread.is_none()
}

/// The rows of message `i` of `pane` for a width of `w` cells.
fn rows(app: &App, pane: &Pane, i: usize, w: usize) -> Vec<Laid> {
    let t = &app.theme;
    let m = &pane.items[i];
    let mut out: Vec<Laid> = Vec::new();
    let new_day = i == 0 || time::day(pane.items[i - 1].ts) != time::day(m.ts);
    if new_day {
        out.push((rule(&time::date(m.ts), w, t.faint()), false));
    }
    let stacked = w < STACKED_BELOW;
    let group = grouped(pane, i);
    let author_style = if m.own { t.own_author() } else { t.author() };
    let (indent_w, text_w) = if stacked {
        (2, w.saturating_sub(2).max(4))
    } else {
        let author_w = AUTHOR_WIDTH.min(w / 4).max(4);
        (author_w, w.saturating_sub(author_w + TIME_WIDTH).max(4))
    };
    let indent = " ".repeat(indent_w);
    if stacked && !group {
        let name = clip(m.author.as_str(), w.saturating_sub(TIME_WIDTH + 2));
        out.push((
            Line::from(vec![
                Span::styled(name, author_style),
                Span::styled(" · ", t.faint()),
                Span::styled(time::hm(m.ts), t.faint()),
            ]),
            false,
        ));
    }
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
        if k == 0 && !stacked && !group {
            let name = clip(m.author.as_str(), indent_w - 1);
            let pad = " ".repeat(indent_w - width(&name));
            spans.push(Span::styled(format!("{name}{pad}"), author_style));
        } else {
            spans.push(Span::raw(indent.clone()));
        }
        let lw = width(&l);
        spans.push(Span::styled(l, t.text()));
        let mut used = lw;
        if m.edited && !edited_row && k + 1 == n {
            used += width(&edited);
            spans.push(Span::styled(edited.clone(), t.muted()));
        }
        if k == 0 && !stacked {
            spans.push(Span::raw(" ".repeat(text_w.saturating_sub(used) + 1)));
            spans.push(Span::styled(time::hm(m.ts), t.faint()));
        }
        out.push((Line::from(spans), false));
    }
    if edited_row {
        out.push((Line::from(vec![Span::raw(indent.clone()), Span::styled(edited, t.muted())]), false));
    }
    if more {
        let label = app.i18n.label(Label::MessageMore).to_string();
        out.push((Line::from(vec![Span::raw(indent.clone()), Span::styled(label, t.muted())]), false));
    }
    if let Some(th) = m.thread.filter(|_| !pane.is_thread()) {
        let msg = Msg::MessageReplies { count: u64::from(th.replies), time: time::hm(th.last_reply) };
        let text = clip(&format!("⤷ {}", app.i18n.msg(&msg)), w.saturating_sub(indent_w));
        out.push((Line::from(vec![Span::raw(indent.clone()), Span::styled(text, t.link())]), true));
    }
    if !m.reactions.is_empty() {
        let mut spans = vec![Span::raw(indent)];
        for (k, r) in m.reactions.iter().enumerate() {
            if k > 0 {
                spans.push(Span::raw("  "));
            }
            let style = if r.mine { t.reaction_mine() } else { t.reaction() };
            spans.push(Span::styled(format!(":{}: {}", r.name, r.count), style));
        }
        out.push((Line::from(spans), false));
    }
    // Below a thread's own message: how many replies follow, or that none do yet.
    if pane.root() == Some(i) {
        let replies = m.thread.map_or(pane.items.len().saturating_sub(1), |t| t.replies as usize);
        if replies > 0 {
            let label = app.i18n.msg(&Msg::ThreadReplies { count: replies as u64 }).to_string();
            out.push((rule(&label, w, t.faint()), false));
        } else if pane.complete {
            out.push((Line::styled(no_replies(app), t.faint()), false));
        }
    }
    LAID_OUT.with(|c| c.set(c.get() + out.len() as u64));
    out
}

/// `── label ──` across `w` cells.
fn rule(label: &str, w: usize, style: ratatui::style::Style) -> Line<'static> {
    let label = format!(" {label} ");
    let left = w.saturating_sub(width(&label)) / 2;
    let right = w.saturating_sub(width(&label) + left);
    Line::styled(format!("{}{label}{}", "─".repeat(left), "─".repeat(right)), style)
}

/// "No replies yet · i reply", with the key bound.
fn no_replies(app: &App) -> String {
    let keys = super::empty::key_of(app, Action::Pane(PaneAction::Insert), Ctx::PaneNormal).unwrap_or_default();
    app.i18n.msg(&Msg::ThreadNoReplies { keys }).to_string()
}

/// Draw `pane`'s messages into `area`; `focused`: the pane has the keyboard (the selection is
/// drawn as the bar of the focused panel, else as the faint one).
pub(super) fn draw(f: &mut Frame, app: &App, pane: &Pane, area: Rect, focused: bool) {
    let t = &app.theme;
    pane.hits.borrow_mut().clear();
    if area.height == 0 || area.width < 3 {
        return;
    }
    // One cell of padding on each side; the left one is the selection's gutter.
    let text_area = |row: Rect| Rect { x: row.x + 1, width: row.width.saturating_sub(2), ..row };
    if pane.items.is_empty() {
        let text = if pane.complete {
            let keys = super::empty::key_of(app, Action::Pane(PaneAction::Insert), Ctx::PaneNormal).unwrap_or_default();
            app.i18n.msg(&Msg::PaneNoMessages { keys }).to_string()
        } else {
            app.i18n.label(Label::PaneLoading).to_string()
        };
        f.render_widget(Paragraph::new(text).style(t.faint()), text_area(area));
        return;
    }
    let (h, w) = (usize::from(area.height), usize::from(area.width.saturating_sub(2)).max(1));
    let n = pane.items.len();
    let mut laid: HashMap<usize, Vec<Laid>> = HashMap::new();
    let height =
        |i: usize, laid: &mut HashMap<usize, Vec<Laid>>| laid.entry(i).or_insert_with(|| rows(app, pane, i, w)).len();
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
    let mut shown: Vec<(usize, Vec<Laid>)> = Vec::new();
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
    // The oldest is on screen with rows to spare and newer messages below the bottom one (after
    // `gg`): those fill the rest, so the history starts on the first row with no gap above it.
    // A conversation that fits whole stays at the bottom, as in any chat; a thread starts at
    // the top.
    let from_top = total < h && (bottom + 1 < n || pane.is_thread());
    let mut i = bottom + 1;
    while total < h && i < n {
        let rows = laid.remove(&i).unwrap_or_else(|| rows(app, pane, i, w));
        total += rows.len();
        shown.push((i, rows));
        i += 1;
    }
    let range = pane.range();
    let how = |i: usize| match (focused, pane.visual.is_some()) {
        (_, true) if pane.selected != Some(i) => Selection::Visual,
        (true, _) => Selection::Focused,
        (false, _) => Selection::Unfocused,
    };
    // Rows cut off at the top when the area is full (at the bottom when filled from the top);
    // rows start at the bottom otherwise.
    let mut skip = if from_top { 0 } else { total.saturating_sub(h) };
    let mut y = if from_top { area.y } else { area.y + h.saturating_sub(total) as u16 };
    let mut hits = pane.hits.borrow_mut();
    for (i, rows) in shown {
        let selected = range.is_some_and(|(a, b)| (a..=b).contains(&i));
        for (line, link) in rows {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if y >= area.bottom() {
                break;
            }
            let row = Rect { y, height: 1, ..area };
            f.render_widget(Paragraph::new(line), text_area(row));
            if selected {
                highlight(f, app, row, how(i));
            }
            hits.push(Hit { y, message: i, link });
            y += 1;
        }
    }
}
