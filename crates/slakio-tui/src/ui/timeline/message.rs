//! A message laid out as rows, in either density (`density`, `:density`).
//!
//! Comfortable (the default, as in GUI Slack): the sender's picture in a 4 × 2 slot on the left
//! ([`crate::avatar::BLOCK`]), the name in bold and the time beside it on the first row, the text
//! under the name using the rest of the width. A message right after one of the same sender
//! (within [`GROUP_SECS`], the same day) leaves the picture and the name out; its time shows in
//! the slot's place while it is selected (GUI Slack shows it on hover).
//!
//! ```text
//! ┌──┐
//!  MK   Minsu Kim  10:02
//!       Starting deploy
//!       ⤷ 4 replies · last 10:15
//!       [chips of the reactions]
//!
//! 10:04 Rolled back                     (grouped; its time while selected)
//! ```
//!
//! Compact: the columns of IRC clients — the chip and the name beside the text, the time on the
//! right ([`compact`]).
//!
//! In both: a message block is its header, its text, the "N replies" link, its reactions as
//! chips. One blank row separates groups, and a message with reactions is followed by one too,
//! so its chips never read as the next message's; never two, and none at the end. A date
//! separator keeps its own row. A pane narrower than [`STACKED_BELOW`] shrinks the picture to the
//! 2-cell chip, and below [`NO_PICTURE_BELOW`] leaves it out.

use crate::action::{Action, PaneAction};
use crate::app::App;
use crate::app::pane::{Pane, Shown};
use crate::app::timelines::Timeline;
use crate::avatar::{self, Art, Slot};
use crate::emoji;
use crate::keymap::Ctx;
use crate::text::{clip, width, wrap};
use crate::time;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use slakio_core::i18n::{Label, Msg};

/// The sender's column, at most (compact).
const AUTHOR_WIDTH: usize = 14;
/// The time column (`10:02` and a space before it, compact).
const TIME_WIDTH: usize = 6;
/// Rows of text a message shows at most; the rest is marked, not drawn.
pub const MAX_TEXT_ROWS: usize = 40;
/// A pane whose inside is narrower than this stacks each message (compact) and shrinks the
/// picture to the chip (comfortable).
pub const STACKED_BELOW: usize = 56;
/// A pane narrower than this shows no picture at all (comfortable).
pub const NO_PICTURE_BELOW: usize = 30;
/// Messages of one sender this close together form a group (GUI Slack groups within a few
/// minutes): the picture and the name show once.
pub const GROUP_SECS: u64 = 5 * 60;

/// What a row is: drawn and selected with its message, a link, a date or divider rule, or the
/// blank between blocks (never selected, never clicked).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Content,
    Link,
    Rule,
    Gap,
}

/// A row laid out.
pub type Laid = (Line<'static>, Kind);

/// Message `i` follows one of the same sender closely: no picture, no name.
pub fn grouped(pane: &Pane, tl: &Timeline, i: usize) -> bool {
    let Some(prev) = i.checked_sub(1).map(|p| &tl.items[p]) else { return false };
    let m = &tl.items[i];
    // A thread's own message and its first reply never group: the divider is between them.
    let root = pane.root(tl);
    prev.user == m.user
        && time::day(prev.ts) == time::day(m.ts)
        && m.ts.0.saturating_sub(prev.ts.0) <= GROUP_SECS * 1_000_000
        && root != Some(i - 1)
        && prev.thread.is_none()
}

/// The rows above message `i`: the blank between blocks (a new group, or after a message with
/// reactions) and the date separator of a new day.
fn above(app: &App, pane: &Pane, tl: &Timeline, i: usize, w: usize) -> Vec<Laid> {
    let m = &tl.items[i];
    let mut out = Vec::new();
    let new_day = i == 0 || time::day(tl.items[i - 1].ts) != time::day(m.ts);
    let after_reactions = i > 0 && !tl.items[i - 1].reactions.is_empty();
    let after_divider = pane.root(tl) == Some(i.wrapping_sub(1));
    if i > 0 && !after_divider && (new_day || after_reactions || !grouped(pane, tl, i)) {
        out.push((Line::raw(""), Kind::Gap));
    }
    if new_day {
        out.push((rule(&time::date(m.ts), w, app.theme.faint()), Kind::Rule));
    }
    out
}

/// The rows of message `i` of `pane` for a width of `w` cells; `selected`: it is selected (a
/// grouped message then shows its time).
pub fn rows(app: &App, pane: &Pane, tl: &Timeline, i: usize, w: usize, selected: bool) -> Vec<Laid> {
    let mut out = above(app, pane, tl, i, w);
    if app.settings.compact {
        out.extend(compact(app, pane, tl, i, w));
    } else {
        out.extend(comfortable(app, pane, tl, i, w, selected));
    }
    out.extend(footer(app, pane, tl, i, w));
    out
}

/// The picture slot a pane `w` cells wide gives a message (none with avatars off).
fn slot_for(app: &App, w: usize) -> Option<Slot> {
    match w {
        _ if !app.settings.avatars => None,
        w if w < NO_PICTURE_BELOW => None,
        w if w < STACKED_BELOW => Some(avatar::CHIP),
        _ => Some(avatar::BLOCK),
    }
}

/// The text of `m` wrapped at `text_w`, emoji drawn, the edited marker on its last line when it
/// fits (else on a row of its own); whether text was left out.
fn text_lines(app: &App, m: &Shown, text_w: usize) -> (Vec<Vec<Span<'static>>>, bool) {
    let t = &app.theme;
    let (lines, more) = wrap(&emoji::replace(m.text.as_str()), text_w, MAX_TEXT_ROWS);
    let mut out: Vec<Vec<Span<'static>>> = lines.into_iter().map(|l| vec![Span::styled(l, t.text())]).collect();
    if m.edited {
        let edited = app.i18n.label(Label::MessageEdited).to_string();
        let last_w: usize = out.last().map_or(0, |l| l.iter().map(Span::width).sum());
        if last_w + 1 + width(&edited) <= text_w {
            if let Some(l) = out.last_mut() {
                l.push(Span::styled(format!(" {edited}"), t.muted()));
            }
        } else {
            out.push(vec![Span::styled(edited, t.muted())]);
        }
    }
    (out, more)
}

/// The comfortable rows of message `i`: header and text beside the picture slot.
fn comfortable(app: &App, pane: &Pane, tl: &Timeline, i: usize, w: usize, selected: bool) -> Vec<Laid> {
    let t = &app.theme;
    let m = &tl.items[i];
    let slot = slot_for(app, w);
    // The text starts after the slot and a gap of 2 (the time of a grouped message fits there).
    let indent = slot.map_or(0, |s| s.cols + 2);
    let text_w = w.saturating_sub(indent).max(4);
    let group = grouped(pane, tl, i);
    let (lines, _) = text_lines(app, m, text_w);
    let mut out: Vec<Laid> = Vec::new();
    if group {
        for (k, mut spans) in lines.into_iter().enumerate() {
            let lead = match (k, selected && indent >= TIME_WIDTH) {
                (0, true) => Span::styled(format!("{:<indent$}", time::hm(m.ts)), t.faint()),
                _ => Span::raw(" ".repeat(indent)),
            };
            spans.insert(0, lead);
            out.push((Line::from(spans), Kind::Content));
        }
        return out;
    }
    let art = Art::Initials(avatar::initials(m.author.as_str(), || {
        app.model.user(&m.user).map(|u| u.name.line().as_str().to_string()).unwrap_or_default()
    }));
    let art_style = t.avatar(avatar::slot(&m.user));
    let picture = |k: usize| -> Vec<Span<'static>> {
        match slot {
            Some(s) if k < s.rows => vec![Span::styled(art.row(s, k), art_style), Span::raw("  ")],
            _ => vec![Span::raw(" ".repeat(indent))],
        }
    };
    let author_style = if m.own { t.own_author() } else { t.author() };
    let time_text = time::hm(m.ts);
    let name = clip(m.author.as_str(), text_w.saturating_sub(width(&time_text) + 2).max(1));
    let mut header = picture(0);
    header.extend([Span::styled(name, author_style), Span::raw("  "), Span::styled(time_text, t.faint())]);
    out.push((Line::from(header), Kind::Content));
    // The slot is as high as it is however short the text: a message without text keeps it.
    let rows = lines.len().max(slot.map_or(1, |s| s.rows) - 1);
    let mut lines = lines.into_iter();
    for k in 0..rows {
        let mut spans = picture(k + 1);
        spans.extend(lines.next().unwrap_or_default());
        out.push((Line::from(spans), Kind::Content));
    }
    out
}

/// The compact rows of message `i`: the chip and the name in a column beside the text, the time
/// on the right; stacked (name and time over the text) in a narrow pane.
fn compact(app: &App, pane: &Pane, tl: &Timeline, i: usize, w: usize) -> Vec<Laid> {
    let t = &app.theme;
    let m = &tl.items[i];
    let stacked = w < STACKED_BELOW;
    let group = grouped(pane, tl, i);
    let author_style = if m.own { t.own_author() } else { t.author() };
    let chip_column = avatar::WIDTH + 1;
    let chip = if group { None } else { super::super::avatar_chip(app, &m.user, m.author.as_str()) };
    let chip_w = if app.settings.avatars { chip_column } else { 0 };
    let stacked_chip = stacked && chip_w > 0 && w.saturating_sub(TIME_WIDTH + 2) > chip_column + 4;
    let (indent_w, text_w) = if stacked {
        let indent = if stacked_chip { chip_column } else { 2 };
        (indent, w.saturating_sub(indent).max(4))
    } else {
        let author_w = AUTHOR_WIDTH.min(w / 4).max(4) + chip_w;
        (author_w, w.saturating_sub(author_w + TIME_WIDTH).max(4))
    };
    let mut out: Vec<Laid> = Vec::new();
    if stacked && !group {
        let mut spans = Vec::new();
        let mut room = w.saturating_sub(TIME_WIDTH + 2);
        if let Some(chip) = chip.clone().filter(|_| stacked_chip) {
            spans.extend([chip, Span::raw(" ")]);
            room -= chip_column;
        }
        spans.extend([
            Span::styled(clip(m.author.as_str(), room), author_style),
            Span::styled(" · ", t.faint()),
            Span::styled(time::hm(m.ts), t.faint()),
        ]);
        out.push((Line::from(spans), Kind::Content));
    }
    let (lines, _) = text_lines(app, m, text_w);
    for (k, line) in lines.into_iter().enumerate() {
        let mut spans: Vec<Span<'static>> = Vec::new();
        if k == 0 && !stacked && !group {
            let mut name_w = indent_w;
            if let Some(chip) = chip.clone() {
                spans.extend([chip, Span::raw(" ")]);
                name_w -= chip_w;
            }
            let name = clip(m.author.as_str(), name_w - 1);
            let pad = " ".repeat(name_w - width(&name));
            spans.push(Span::styled(format!("{name}{pad}"), author_style));
        } else {
            spans.push(Span::raw(" ".repeat(indent_w)));
        }
        let used: usize = line.iter().map(Span::width).sum();
        spans.extend(line);
        if k == 0 && !stacked {
            spans.push(Span::raw(" ".repeat(text_w.saturating_sub(used) + 1)));
            spans.push(Span::styled(time::hm(m.ts), t.faint()));
        }
        out.push((Line::from(spans), Kind::Content));
    }
    out
}

/// The text's column in the current density, for the rows under it.
fn indent(app: &App, w: usize) -> usize {
    if app.settings.compact {
        let chip_w = if app.settings.avatars { avatar::WIDTH + 1 } else { 0 };
        if w < STACKED_BELOW {
            if chip_w > 0 && w > chip_w + TIME_WIDTH + 6 { chip_w } else { 2 }
        } else {
            AUTHOR_WIDTH.min(w / 4).max(4) + chip_w
        }
    } else {
        slot_for(app, w).map_or(0, |s| s.cols + 2)
    }
}

/// The rows under a message's text: the cut marker, the "N replies" link, the reactions as
/// chips, and below a thread's own message the divider (or that no reply came yet).
fn footer(app: &App, pane: &Pane, tl: &Timeline, i: usize, w: usize) -> Vec<Laid> {
    let t = &app.theme;
    let m = &tl.items[i];
    let indent_w = indent(app, w);
    let indent = " ".repeat(indent_w);
    let mut out = Vec::new();
    let text_w = w.saturating_sub(indent_w).max(4);
    if wrap(&emoji::replace(m.text.as_str()), text_w, MAX_TEXT_ROWS).1 {
        let label = app.i18n.label(Label::MessageMore).to_string();
        out.push((Line::from(vec![Span::raw(indent.clone()), Span::styled(label, t.muted())]), Kind::Content));
    }
    if let Some(th) = m.thread.filter(|_| !pane.is_thread()) {
        let msg = Msg::MessageReplies { count: u64::from(th.replies), time: time::hm(th.last_reply) };
        let text = clip(&format!("⤷ {}", app.i18n.msg(&msg)), w.saturating_sub(indent_w));
        out.push((Line::from(vec![Span::raw(indent.clone()), Span::styled(text, t.link())]), Kind::Link));
    }
    for line in chips(app, m, text_w) {
        let mut spans = vec![Span::raw(indent.clone())];
        spans.extend(line);
        out.push((Line::from(spans), Kind::Content));
    }
    if pane.root(tl) == Some(i) {
        let replies = m.thread.map_or(tl.items.len().saturating_sub(1), |t| t.replies as usize);
        if replies > 0 {
            let label = app.i18n.msg(&Msg::ThreadReplies { count: replies as u64 }).to_string();
            out.push((rule(&label, w, t.faint()), Kind::Rule));
        } else if tl.complete {
            out.push((Line::styled(no_replies(app), t.faint()), Kind::Rule));
        }
    }
    out
}

/// The reactions of `m` as chips ` <emoji> <count> ` (the user's own in the accent), one space
/// apart, wrapped at `w` cells. An emoji Slack names that is not a standard one stays `:name:`.
pub fn chips(app: &App, m: &Shown, w: usize) -> Vec<Vec<Span<'static>>> {
    let t = &app.theme;
    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    let mut used = 0;
    for r in &m.reactions {
        let name = r.name.as_str();
        let face = emoji::get(name).map_or_else(|| format!(":{name}:"), str::to_string);
        let chip = format!(" {face} {} ", r.count);
        let cw = width(&chip);
        let style: Style = if r.mine { t.reaction_mine() } else { t.reaction() };
        if lines.is_empty() || (used > 0 && used + 1 + cw > w) {
            lines.push(Vec::new());
            used = 0;
        }
        let line = lines.last_mut().expect("a line was pushed");
        if used > 0 {
            line.push(Span::raw(" "));
            used += 1;
        }
        line.push(Span::styled(clip(&chip, w.max(1)), style));
        used += cw;
    }
    lines
}

/// `── label ──` across `w` cells.
pub fn rule(label: &str, w: usize, style: Style) -> Line<'static> {
    let label = format!(" {label} ");
    let left = w.saturating_sub(width(&label)) / 2;
    let right = w.saturating_sub(width(&label) + left);
    Line::styled(format!("{}{label}{}", "─".repeat(left), "─".repeat(right)), style)
}

/// "No replies yet · i reply", with the key bound.
fn no_replies(app: &App) -> String {
    let keys = super::super::empty::key_of(app, Action::Pane(PaneAction::Insert), Ctx::PaneNormal).unwrap_or_default();
    app.i18n.msg(&Msg::ThreadNoReplies { keys }).to_string()
}
