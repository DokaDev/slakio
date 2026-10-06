//! The status line, lualine-style: the mode badge, where you are, a notice (or the first keys of
//! a sequence), and on the right the hint line and the backend's state, the segments apart by
//! ` │ `. Unread counts are the view switcher's, never repeated here.
//!
//! ```text
//!  NORMAL  ▌A company › #backend │ Copied 3 messages │ i write · k messages · ? help │ demo
//! ```
//!
//! The hint line shows the keys worth knowing where the keyboard is ([`crate::keymap::hints`]),
//! read from the key map. When the line is short, parts go in this order: the workspace's name
//! (its stripe stays), hints from the last, the middle of the place. The badge and `demo` always
//! stay.

use super::view_label;
use super::work::breadcrumb;
use crate::app::model::Row;
use crate::app::status::Level;
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
        Ctx::ViewSwitcher => Place::ViewSwitcher,
        Ctx::List => match app.shell.rows(&app.model).get(app.shell.list_cursor) {
            Some(Row::Section(_)) => Place::ListSection,
            Some(_) => Place::ListConversation,
            None => Place::ListEmpty,
        },
        Ctx::ComposerInsert => Place::Insert,
        Ctx::PaneVisual => Place::Visual,
        _ => {
            let panel = app.work.active().is_some_and(|id| app.work.owner(id).is_some());
            match app.work.focused() {
                None => Place::WorkEmpty,
                Some(p) if p.selected.is_some() && !p.is_thread() => Place::PaneSelected,
                Some(p) if p.is_thread() && p.selected.is_some() => Place::ThreadSelected,
                Some(p) if p.is_thread() && panel => Place::Thread,
                Some(p) if p.is_thread() => Place::ThreadPane,
                Some(_) => Place::Pane,
            }
        }
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
    let main = app.work.home().and_then(|p| app.model.target(&p.target));
    let view = app.i18n.label(view_label(app.shell.view)).to_string();
    // With the list panel out of the layout, its view switcher is too: the place names the view.
    let view_hidden = app.backend.is_some() && !crate::screen::too_small(app.size) && app.areas().list.is_none();
    let mut place_text = app.backend.map(|_| {
        let place = match main {
            Some(c) if app.work.focused().is_some_and(|p| p.is_thread()) => {
                format!("{} › ⤷ {}", breadcrumb(c), app.i18n.label(Label::PaneThread))
            }
            Some(c) => breadcrumb(c),
            None => return view.clone(),
        };
        if view_hidden { format!("{view} › {place}") } else { place }
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
    let right = |hints: &[(String, String)]| -> Spans {
        let mut out: Spans = Vec::new();
        for part in [hint_spans(hints)] {
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
    let need = |ws_name: &Option<String>, place_text: &Option<String>, hints: &[(String, String)]| {
        // A notice is worth more than the hints: they make room for it.
        let msg_w = msg.as_ref().map_or(0, |(m, _)| 3 + width(m).min(48));
        spans_width(&left(ws_name, place_text)) + msg_w + spans_width(&right(hints))
    };
    // Shorten until it fits, in this order: the hints of least worth (a peek, the command line,
    // the next pane, the view switcher), the workspace's name (cut with `…`, eight cells kept), the other hints by
    // worth, the middle of the place, the place's first key and help, the name. Then what is left is filled again: a dropped hint comes back where cutting
    // the name makes room for it, and the name grows into the rest, so no run of blank cells is
    // left between the two sides.
    let over = |ws: &Option<String>, pl: &Option<String>, h: &[usize]| {
        let pick: Vec<(String, String)> = h.iter().map(|&j| all_hints[j].clone()).collect();
        need(ws, pl, &pick).saturating_sub(w)
    };
    let full_name = ws_name.clone();
    // The name cut by `by` cells, never under eight (or its own width).
    let cut = |by: usize| -> Option<String> {
        full_name.as_ref().map(|n| clip(n, width(n).saturating_sub(by).max(8.min(width(n)))))
    };
    let name_room = full_name.as_ref().map_or(0, |n| width(n).saturating_sub(8));
    let low = |l: Label| matches!(l, Label::HintPeek | Label::HintCommands | Label::HintNextPane | Label::HintNav);
    let low_order: Vec<usize> = [Label::HintPeek, Label::HintCommands, Label::HintNextPane, Label::HintNav]
        .iter()
        .filter_map(|l| resolved.iter().position(|(_, x)| x == l))
        .collect();
    // A hint's worth: help most, then the place's first key (what to press now), the leader key
    // and the new tab key (the way to tabs), the others, those of least worth.
    let worth = |i: usize| match resolved[i].1 {
        Label::HintHelp => 5,
        _ if i == 0 => 4,
        Label::HintMore | Label::HintNewTab => 3,
        l if low(l) => 1,
        _ => 2,
    };
    let rest_order: Vec<usize> = (2..=5)
        .flat_map(|k| (0..resolved.len()).rev().filter(move |&i| worth(i) == k))
        .filter(|&i| !low(resolved[i].1))
        .collect();
    let mut chosen: Vec<usize> = (0..resolved.len()).collect();
    for &i in &low_order {
        if over(&ws_name, &place_text, &chosen) == 0 {
            break;
        }
        chosen.retain(|&j| j != i);
    }
    let excess = over(&ws_name, &place_text, &chosen);
    if excess > 0 {
        ws_name = cut(excess);
    }
    for &i in &rest_order {
        if over(&ws_name, &place_text, &chosen) == 0 {
            break;
        }
        // The place gives up its middle before the first key and help go.
        if worth(i) >= 4
            && let Some(p) = place_text.as_mut()
        {
            let excess = over(&ws_name, &Some(p.clone()), &chosen);
            *p = clip_middle(p, width(p).saturating_sub(excess).max(10));
            if over(&ws_name, &place_text, &chosen) == 0 {
                break;
            }
        }
        chosen.retain(|&j| j != i);
    }
    if let Some(p) = place_text.as_mut() {
        let excess = over(&ws_name, &Some(p.clone()), &chosen);
        if excess > 0 {
            *p = clip_middle(p, width(p).saturating_sub(excess).max(4));
        }
    }

    if over(&ws_name, &place_text, &chosen) > 0 {
        ws_name = None;
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
            let fits = need(&ws_name, &place_text, &pick) <= w;
            fits.then(|| (need(&full_name, &place_text, &pick).min(w + cut_room), 0))
        };
        for mask in 0..1usize << n {
            let with: Vec<usize> = (0..n).filter(|&i| mask >> i & 1 == 1).collect();
            // Help comes before any other hint, and the place's first key before the rest.
            let needs = |k: usize| (0..n).any(|j| worth(j) == k && !with.contains(&j));
            // A hint of least worth only comes with the others of its place.
            // The new tab key comes before the other hints but a peek (the way to tabs is taught;
            // a peek is the row's other way to open).
            let left_out = |l: Label| (0..n).any(|j| resolved[j].1 == l && !with.contains(&j));
            let tab_left_out = left_out(Label::HintNewTab);
            // The way to the view switcher comes before the next pane, which leads there too.
            let has = |l: Label| with.iter().any(|&j| resolved[j].1 == l);
            let broken = (!with.is_empty() && needs(5))
                || (with.iter().any(|&i| worth(i) < 4) && needs(4))
                || (with.iter().any(|&i| worth(i) <= 2 && resolved[i].1 != Label::HintPeek) && tab_left_out)
                || (has(Label::HintNextPane) && left_out(Label::HintNav))
                || (with.iter().any(|&i| worth(i) == 1) && needs(2));
            if broken {
                continue;
            }
            if over(&full_name, &place_text, &with) > cut_room {
                continue;
            }
            let pick: Vec<(String, String)> = with.iter().map(|&j| all_hints[j].clone()).collect();
            let filled = need(&full_name, &place_text, &pick).min(w + cut_room);
            let value: usize = with.iter().map(|&i| 1 << worth(i)).sum();
            if best.is_none_or(|(f, v)| filled > f || (filled == f && value > v)) {
                best = Some((filled, value));
                chosen = with;
            }
        }
        ws_name = cut(over(&full_name, &place_text, &chosen));
    }
    let hints: Vec<(String, String)> = chosen.iter().map(|&j| all_hints[j].clone()).collect();
    let mut line = left(&ws_name, &place_text);
    let right_spans = right(&hints);
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
