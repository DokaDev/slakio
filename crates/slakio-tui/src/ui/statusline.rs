//! The status line, lualine-style: the mode badge, where you are, a notice (or the first keys of
//! a sequence), and on the right the hint line, the unread totals and the backend's state, the
//! segments apart by ` │ `.
//!
//! ```text
//!  NORMAL  ▌A company › #backend │ Copied 3 messages │ i write · k messages · ? help │ 3 DM 2 │ demo
//! ```
//!
//! The hint line shows the keys worth knowing where the keyboard is ([`crate::keymap::hints`]),
//! read from the key map. When the line is short, parts go in this order: the workspace's name
//! (its stripe stays), hints from the last, the middle of the place, the DM count, the mention
//! count. The badge and `demo` always stay.

use super::work::breadcrumb;
use super::{count_pill, view_glyph, view_label};
use crate::app::model::Row;
use crate::app::shell::View;
use crate::app::status::Level;
use crate::app::work::Side;
use crate::app::{App, Mode};
use crate::keymap::hints::{self, Place};
use crate::keymap::{Ctx, keys};
use crate::text::{clip, width};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::i18n::Label;
use std::time::Instant;

/// Where the keyboard is, for the hint line (`None` under a popup with keys of its own).
pub(super) fn place(app: &App) -> Option<(Place, Ctx)> {
    if app.dialog.is_some() || app.help.is_some() {
        return None;
    }
    let ctx = app.screen_context();
    let place = match ctx {
        Ctx::CommandLine => Place::CommandLine,
        Ctx::Root => Place::Welcome,
        Ctx::Rail => Place::Rail,
        Ctx::List => match app.shell.rows(&app.model).get(app.shell.list_cursor) {
            Some(Row::Section(_)) => Place::ListSection,
            Some(_) => Place::ListConversation,
            None => Place::ListEmpty,
        },
        Ctx::ComposerInsert => Place::Insert,
        Ctx::PaneVisual => Place::Visual,
        _ => match app.work.focused() {
            None => Place::WorkEmpty,
            Some(p) if p.selected.is_some() && app.work.side == Side::Main => Place::PaneSelected,
            Some(p) if app.work.side == Side::Thread && p.selected.is_some() => Place::ThreadSelected,
            Some(_) if app.work.side == Side::Thread => Place::Thread,
            Some(_) => Place::Pane,
        },
    };
    Some((place, ctx))
}

type Spans = Vec<Span<'static>>;

fn spans_width(s: &[Span]) -> usize {
    s.iter().map(Span::width).sum()
}

/// The mode badge at the left of the status line, and a space after it.
fn badge_spans(app: &App) -> Spans {
    let t = &app.theme;
    let (label, bg) = match app.mode() {
        Mode::Normal => (Label::ModeNormal, t.mode_normal),
        Mode::Insert => (Label::ModeInsert, t.mode_insert),
        Mode::Visual => (Label::ModeVisual, t.mode_visual),
        Mode::Command => (Label::ModeCommand, t.mode_command),
    };
    vec![Span::styled(format!(" {} ", app.i18n.label(label)), t.mode(bg)), Span::raw(" ")]
}

/// The mode badge alone, drawn again over a dimmed screen (it stays bright under the palette).
pub(super) fn badge(f: &mut Frame, app: &App, area: Rect) {
    let spans = badge_spans(app);
    let w = spans.first().map_or(0, Span::width) as u16;
    f.render_widget(Paragraph::new(Line::from(spans)), Rect { width: w.min(area.width), ..area });
}

#[expect(
    clippy::too_many_lines,
    reason = "the hint fitting is still inside the draw; it moves out into a pure function"
)]
pub(super) fn draw(f: &mut Frame, app: &App, area: Rect, now: Instant) {
    let t = &app.theme;
    let w = usize::from(area.width);
    f.buffer_mut().set_style(area, t.surface());
    let badge = badge_spans(app);
    let sep = || Span::styled(" │ ", t.divider());

    // The hints, best first.
    // The which-key popup lists the keys itself.
    let resolved: Vec<(String, Label)> = place(app)
        .filter(|_| !app.which_key_visible(now))
        .map(|(p, ctx)| hints::resolve(&app.keymap, p, ctx))
        .unwrap_or_default();
    let all_hints: Vec<(String, String)> =
        resolved.iter().map(|(k, l)| (k.clone(), app.i18n.label(*l).to_string())).collect();
    let hint_spans = |hints: &[(String, String)]| -> Spans {
        let mut out = Vec::new();
        for (i, (k, l)) in hints.iter().enumerate() {
            if i > 0 {
                out.push(Span::styled(" · ", t.faint()));
            }
            out.push(Span::styled(k.clone(), t.bold()));
            out.push(Span::styled(format!(" {l}"), t.muted()));
        }
        out
    };

    // Where you are.
    let ws = app.backend.and_then(|_| app.model.workspaces().get(app.shell.workspace));
    let mut ws_name = ws.map(|w| w.name.line().into_string());
    let stripe = ws.map(|w| Span::styled("▌", t.workspace(w.color)));
    let main = app.work.main.as_ref().and_then(|p| app.model.target(&p.target));
    let mut place_text = app.backend.map(|_| match main {
        Some(c) if app.work.side == Side::Thread && app.work.thread.is_some() => {
            format!("{} › ⤷ {}", breadcrumb(c), app.i18n.label(Label::PaneThread))
        }
        Some(c) => breadcrumb(c),
        None => app.i18n.label(view_label(app.shell.view)).to_string(),
    });
    // The notice, or the first keys of a sequence until its popup shows.
    let msg: Option<(String, Style)> = if !app.keys.pending().is_empty() {
        (!app.which_key_visible(now)).then(|| (format!("{} …", keys::label(app.keys.pending())), t.bold()))
    } else {
        app.status.notice(now).map(|n| {
            let style = if n.level == Level::Warning { t.warning() } else { t.text() };
            (app.i18n.msg(&n.msg).to_string(), style)
        })
    };
    // Unread totals.
    let icons = app.settings.icons;
    let mut mentions = app.backend.map_or(0, |_| app.model.mentions());
    let mut dms = app.backend.map_or(0, |_| app.model.dm_unread(app.shell.workspace));
    let demo = app.backend.filter(|c| c.demo).map(|_| app.i18n.label(Label::StatusDemo).to_string());

    let left = |ws_name: &Option<String>, place_text: &Option<String>| -> Spans {
        let mut out = badge.clone();
        if let Some(s) = &stripe {
            out.push(s.clone());
        }
        if let Some(n) = ws_name {
            out.push(Span::styled(n.clone(), t.text()));
        }
        if let Some(p) = place_text {
            let lead = match (ws_name.is_some(), stripe.is_some()) {
                (true, _) => " › ",
                // The stripe alone stands right before the place.
                (false, true) => "",
                (false, false) => " ",
            };
            out.push(Span::styled(lead, t.faint()));
            out.push(Span::styled(p.clone(), t.text()));
        }
        out
    };
    let counts = |mentions: u32, dms: u32| -> Spans {
        let mut out = Vec::new();
        if mentions > 0 {
            if icons {
                out.push(Span::styled(format!("{} ", view_glyph(View::Activity, true)), t.muted()));
            }
            let pill = if icons { count_pill(mentions) } else { format!(" @{} ", count_pill(mentions).trim()) };
            out.push(Span::styled(pill, t.badge()));
        }
        if dms > 0 {
            let name = if icons {
                view_glyph(View::Dms, true).to_string()
            } else {
                app.i18n.label(Label::StatusDms).to_string()
            };
            if !out.is_empty() {
                out.push(Span::raw(" "));
            }
            out.push(Span::styled(format!("{name} {dms}"), t.bold()));
        }
        out
    };
    let right = |hints: &[(String, String)], mentions: u32, dms: u32| -> Spans {
        let mut out: Spans = Vec::new();
        for part in [hint_spans(hints), counts(mentions, dms)] {
            if part.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push(sep());
            }
            out.extend(part);
        }
        if let Some(d) = &demo {
            if !out.is_empty() {
                out.push(sep());
            }
            out.push(Span::styled(d.clone(), t.warm()));
        }
        // Apart from the place (and a notice) on the left.
        if !out.is_empty() {
            out.insert(0, sep());
        }
        out.push(Span::raw(" "));
        out
    };
    let need = |ws_name: &Option<String>, place_text: &Option<String>, hints: &[(String, String)], m: u32, d: u32| {
        // A notice is worth more than the hints: they make room for it.
        let msg_w = msg.as_ref().map_or(0, |(m, _)| 3 + width(m).min(48));
        spans_width(&left(ws_name, place_text)) + msg_w + spans_width(&right(hints, m, d))
    };
    // Shorten until it fits, in this order: the hints of least worth (a peek, the command line,
    // the next pane, the rail), the workspace's name (cut with `…`, eight cells kept), the other hints by
    // worth, the middle of the place, the place's first key and help, the name, the DM count, the
    // mentions. Then what is left is filled again: a dropped hint comes back where cutting
    // the name makes room for it, and the name grows into the rest, so no run of blank cells is
    // left between the two sides.
    let over = |ws: &Option<String>, pl: &Option<String>, h: &[usize], m: u32, d: u32| {
        let pick: Vec<(String, String)> = h.iter().map(|&j| all_hints[j].clone()).collect();
        need(ws, pl, &pick, m, d).saturating_sub(w)
    };
    let full_name = ws_name.clone();
    // The name cut by `by` cells, never under eight (or its own width).
    let cut = |by: usize| -> Option<String> {
        full_name.as_ref().map(|n| clip(n, width(n).saturating_sub(by).max(8.min(width(n)))))
    };
    let name_room = full_name.as_ref().map_or(0, |n| width(n).saturating_sub(8));
    let low = |l: Label| matches!(l, Label::HintPeek | Label::HintCommands | Label::HintNextPane | Label::HintRail);
    let low_order: Vec<usize> = [Label::HintPeek, Label::HintCommands, Label::HintNextPane, Label::HintRail]
        .iter()
        .filter_map(|l| resolved.iter().position(|(_, x)| x == l))
        .collect();
    // A hint's worth: help most, then the place's first key (what to press now), the leader key,
    // the others, those of least worth.
    let worth = |i: usize| match resolved[i].1 {
        Label::HintHelp => 5,
        _ if i == 0 => 4,
        Label::HintMore => 3,
        l if low(l) => 1,
        _ => 2,
    };
    let rest_order: Vec<usize> = (2..=5)
        .flat_map(|k| (0..resolved.len()).rev().filter(move |&i| worth(i) == k))
        .filter(|&i| !low(resolved[i].1))
        .collect();
    let mut chosen: Vec<usize> = (0..resolved.len()).collect();
    for &i in &low_order {
        if over(&ws_name, &place_text, &chosen, mentions, dms) == 0 {
            break;
        }
        chosen.retain(|&j| j != i);
    }
    let excess = over(&ws_name, &place_text, &chosen, mentions, dms);
    if excess > 0 {
        ws_name = cut(excess);
    }
    for &i in &rest_order {
        if over(&ws_name, &place_text, &chosen, mentions, dms) == 0 {
            break;
        }
        // The place gives up its middle before the first key and help go.
        if worth(i) >= 4
            && let Some(p) = place_text.as_mut()
        {
            let excess = over(&ws_name, &Some(p.clone()), &chosen, mentions, dms);
            *p = clip_middle(p, width(p).saturating_sub(excess).max(10));
            if over(&ws_name, &place_text, &chosen, mentions, dms) == 0 {
                break;
            }
        }
        chosen.retain(|&j| j != i);
    }
    if let Some(p) = place_text.as_mut() {
        let excess = over(&ws_name, &Some(p.clone()), &chosen, mentions, dms);
        if excess > 0 {
            *p = clip_middle(p, width(p).saturating_sub(excess).max(4));
        }
    }

    if over(&ws_name, &place_text, &chosen, mentions, dms) > 0 {
        ws_name = None;
    }
    if over(&ws_name, &place_text, &chosen, mentions, dms) > 0 {
        dms = 0;
    }
    if over(&ws_name, &place_text, &chosen, mentions, dms) > 0 {
        mentions = 0;
    }
    // The name is cut for more hints only where it had to be cut anyway.
    let cut_room = if ws_name == full_name { 0 } else { name_room };
    if ws_name.is_some() {
        // Of the sets of hints that fit (cutting the name for them, never under eight cells),
        // the one that fills the line best, so a hint that fits where blanks would be is shown;
        // help and the place's first key come first. Of equal fills, the one worth more.
        let n = resolved.len();
        let mut best: Option<(usize, usize)> = {
            // What dropping gave, unless a set below fills more.
            let pick: Vec<(String, String)> = chosen.iter().map(|&j| all_hints[j].clone()).collect();
            let fits = need(&ws_name, &place_text, &pick, mentions, dms) <= w;
            fits.then(|| (need(&full_name, &place_text, &pick, mentions, dms).min(w + cut_room), 0))
        };
        for mask in 0..1usize << n {
            let with: Vec<usize> = (0..n).filter(|&i| mask >> i & 1 == 1).collect();
            // Help comes before any other hint, and the place's first key before the rest.
            let needs = |k: usize| (0..n).any(|j| worth(j) == k && !with.contains(&j));
            // A hint of least worth only comes with the others of its place.
            let broken = (!with.is_empty() && needs(5))
                || (with.iter().any(|&i| worth(i) < 4) && needs(4))
                || (with.iter().any(|&i| worth(i) == 1) && needs(2));
            if broken {
                continue;
            }
            if over(&full_name, &place_text, &with, mentions, dms) > cut_room {
                continue;
            }
            let pick: Vec<(String, String)> = with.iter().map(|&j| all_hints[j].clone()).collect();
            let filled = need(&full_name, &place_text, &pick, mentions, dms).min(w + cut_room);
            let value: usize = with.iter().map(|&i| 1 << worth(i)).sum();
            if best.is_none_or(|(f, v)| filled > f || (filled == f && value > v)) {
                best = Some((filled, value));
                chosen = with;
            }
        }
        ws_name = cut(over(&full_name, &place_text, &chosen, mentions, dms));
    }
    let hints: Vec<(String, String)> = chosen.iter().map(|&j| all_hints[j].clone()).collect();
    let mut line = left(&ws_name, &place_text);
    let right_spans = right(&hints, mentions, dms);
    if let Some((m, style)) = msg {
        let room = w.saturating_sub(spans_width(&line) + spans_width(&right_spans) + 3);
        if room > 0 {
            line.push(sep());
            line.push(Span::styled(clip(&m, room), style));
        }
    }
    f.render_widget(Paragraph::new(Line::from(line)), area);
    draw_right(f, area, right_spans);
}

/// `spans` at the right end of `area`, when they fit.
fn draw_right(f: &mut Frame, area: Rect, spans: Spans) {
    let w = spans_width(&spans) as u16;
    if w == 0 || w >= area.width {
        return;
    }
    let at = Rect { x: area.right() - w, width: w, ..area };
    f.render_widget(Paragraph::new(Line::from(spans)), at);
}

/// `s` cut to `w` cells by its middle: `#feed-cu…-16`.
fn clip_middle(s: &str, w: usize) -> String {
    if width(s) <= w {
        return s.to_string();
    }
    let head = clip(s, w.div_ceil(2));
    let tail_w = w.saturating_sub(width(&head));
    let chars: Vec<char> = s.chars().collect();
    let mut tail = String::new();
    for c in chars.iter().rev() {
        let mut next = c.to_string();
        next.push_str(&tail);
        if width(&next) > tail_w {
            break;
        }
        tail = next;
    }
    format!("{head}{tail}")
}

#[cfg(test)]
mod tests {
    use super::clip_middle;

    #[test]
    fn a_long_place_is_cut_in_its_middle() {
        assert_eq!(clip_middle("#feed-customer-16", 10), "#fee…er-16");
        assert_eq!(clip_middle("#backend", 10), "#backend");
    }
}
