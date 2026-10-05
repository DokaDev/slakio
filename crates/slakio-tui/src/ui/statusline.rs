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
use ratatui::layout::{Position, Rect};
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
            _ => Place::ListConversation,
        },
        Ctx::ComposerInsert => Place::Insert,
        Ctx::PaneVisual => Place::Visual,
        _ => match app.work.focused() {
            None => Place::WorkEmpty,
            Some(p) if p.selected.is_some() => Place::PaneSelected,
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

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect, now: Instant) {
    let t = &app.theme;
    let w = usize::from(area.width);
    f.buffer_mut().set_style(area, t.surface());
    let (label, bg) = match app.mode() {
        Mode::Normal => (Label::ModeNormal, t.mode_normal),
        Mode::Insert => (Label::ModeInsert, t.mode_insert),
        Mode::Visual => (Label::ModeVisual, t.mode_visual),
        Mode::Command => (Label::ModeCommand, t.mode_command),
    };
    let badge: Spans = vec![Span::styled(format!(" {} ", app.i18n.label(label)), t.mode(bg)), Span::raw(" ")];
    let sep = || Span::styled(" │ ", t.divider());

    // The hints, best first.
    let mut hints: Vec<(String, String)> = place(app)
        .map(|(p, ctx)| hints::resolve(&app.keymap, p, ctx))
        .unwrap_or_default()
        .into_iter()
        .map(|(k, l)| (k, app.i18n.label(l).to_string()))
        .collect();
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

    if app.mode() == Mode::Command {
        let text = format!(":{}", app.cmdline.text());
        let left_w = spans_width(&badge) + width(&text);
        while !hints.is_empty() && left_w + 2 + spans_width(&hint_spans(&hints)) > w {
            hints.pop();
        }
        let mut line = badge;
        line.push(Span::styled(text, t.text()));
        f.render_widget(Paragraph::new(Line::from(line)), area);
        draw_right(f, area, hint_spans(&hints));
        let x = area.x + left_w as u16;
        f.set_cursor_position(Position { x: x.min(area.right().saturating_sub(1)), y: area.y });
        return;
    }

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
            let lead = if ws_name.is_some() { " › " } else { " " };
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
        out.push(Span::raw(" "));
        out
    };
    let need = |ws_name: &Option<String>, place_text: &Option<String>, hints: &[(String, String)], m: u32, d: u32| {
        // A notice is worth more than the hints: they make room for it.
        let msg_w = msg.as_ref().map_or(0, |(m, _)| 3 + width(m).min(48));
        spans_width(&left(ws_name, place_text)) + msg_w + 1 + spans_width(&right(hints, m, d))
    };
    // Shorten in the order of the module docs until it fits.
    if need(&ws_name, &place_text, &hints, mentions, dms) > w {
        ws_name = None;
    }
    while !hints.is_empty() && need(&ws_name, &place_text, &hints, mentions, dms) > w {
        hints.pop();
    }
    if let Some(p) = place_text.as_mut() {
        let over = need(&ws_name, &Some(p.clone()), &hints, mentions, dms).saturating_sub(w);
        if over > 0 {
            *p = clip_middle(p, width(p).saturating_sub(over).max(4));
        }
    }
    if need(&ws_name, &place_text, &hints, mentions, dms) > w {
        dms = 0;
    }
    if need(&ws_name, &place_text, &hints, mentions, dms) > w {
        mentions = 0;
    }
    let mut line = left(&ws_name, &place_text);
    let right_spans = right(&hints, mentions, dms);
    if let Some((m, style)) = msg {
        let room = w.saturating_sub(spans_width(&line) + spans_width(&right_spans) + 4);
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
