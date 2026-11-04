//! A message's text as drawn: its lines of Slack markup ([`crate::mrkdwn`]) as rows of styled
//! spans at a width. Text wraps at blanks; a quoted line wraps under a bar; a code block's line is
//! never wrapped, it is cut with `…`. People and channels are named from the read model when it
//! knows them (their names may come after the message), else by the label Slack wrote, else by
//! their id. The user named, and everyone (`@here`), stand out as GUI Slack marks a mention; an
//! app named is marked. A link that opens (a web address, [`crate::opener::check`]) is
//! underlined; every other one (`javascript:`, `mailto:`, `file:` …) is plain text. Where a
//! link's label reads as another address than its own, the real target follows it in a faint
//! span of its own (`→ host`), a style no sender's markup gives. While a link is being picked
//! (`o`), the letters still to type for it are drawn over its first cells in the hint style. Inline code is tinted, or keeps
//! its backticks where nothing can tint it; a code block's lines are tinted, or marked by a bar.

use crate::app::App;
use crate::app::pane::Shown;
use crate::mrkdwn::{Block, Marks, Piece, plain, real_target};
use crate::text::{Spot, clip, grapheme_width, width, wrap_tagged};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use slakio_core::model::WorkspaceId;
use unicode_segmentation::UnicodeSegmentation;

/// The bar before a quoted line, and the blank after it.
const QUOTE: &str = "▎ ";

/// The bar before a code block's line where nothing tints it (no color), and the blank after it.
const CODE: &str = "│ ";

/// The rows of a message's text, and on each where its links are ([`Spot`]: cells from the
/// row's start, the link's number among the message's links, from 0).
pub type Rows = (Vec<Vec<Span<'static>>>, Vec<Vec<Spot>>);

/// The rows of `m`'s text in a pane of workspace `ws`, `w` cells wide, at most `max` of them;
/// whether text was left out. `hints`: the letters still to type for each link, by its number,
/// while one is being picked (else empty).
pub fn rows(app: &App, ws: &WorkspaceId, m: &Shown, w: usize, max: usize, hints: &[Option<String>]) -> (Rows, bool) {
    let t = &app.theme;
    let mut out: Vec<Vec<Span<'static>>> = Vec::new();
    let mut spots: Vec<Vec<Spot>> = Vec::new();
    let mut link = 0;
    let mut placed = std::collections::HashSet::new();
    for (k, block) in m.body.iter().enumerate() {
        let room = max.saturating_sub(out.len());
        if room == 0 {
            return ((out, spots), true);
        }
        spots.resize(out.len(), Vec::new());
        match block {
            // Without color nothing tints a code block: a bar marks its lines.
            Block::Code(line) if t.kind == crate::theme::Kind::NoColor => {
                out.push(vec![Span::styled(CODE, t.faint()), Span::styled(clip(line, w.saturating_sub(2)), t.code())]);
            }
            Block::Code(line) => out.push(vec![Span::styled(clip(line, w), t.code())]),
            Block::Line { quote, pieces } => {
                let (mut runs, mut tags) = (Vec::new(), Vec::new());
                for p in pieces {
                    let of_link = matches!(p, Piece::Link { .. }).then_some(link);
                    for r in runs_of(app, ws, p) {
                        runs.push(r);
                        tags.push(of_link);
                    }
                    link += usize::from(of_link.is_some());
                }
                let inner = if *quote { w.saturating_sub(2) } else { w };
                let (lines, on, more) = wrap_tagged(&runs, &tags, inner, room, t.text());
                for (mut line, mut on) in lines.into_iter().zip(on) {
                    // The letters go over a link's first cells: nothing moves or wraps again.
                    for &(a, _, i) in &on {
                        if let Some(hint) = hints.get(i).and_then(Option::as_deref).filter(|_| placed.insert(i)) {
                            line = over(line, a, hint, t.hint());
                        }
                    }
                    if *quote {
                        line.insert(0, Span::styled(QUOTE, t.faint()));
                        on.iter_mut().for_each(|s| (s.0, s.1) = (s.0 + 2, s.1 + 2));
                    }
                    out.push(line);
                    spots.push(on);
                }
                if more {
                    return ((out, spots), true);
                }
            }
        }
        spots.resize(out.len(), Vec::new());
        if out.len() >= max && k + 1 < m.body.len() {
            return ((out, spots), true);
        }
    }
    ((out, spots), false)
}

/// `line` with `text` drawn in `style` over its cells from `at` on (a wide character it covers
/// in part leaves blanks): the line keeps its width. Past the line's end the text goes after it.
fn over(line: Vec<Span<'static>>, at: usize, text: &str, style: Style) -> Vec<Span<'static>> {
    let end = at + width(text);
    let mut out: Vec<Span<'static>> = Vec::new();
    let put = |out: &mut Vec<Span<'static>>, s: &str, st: Style| match out.last_mut() {
        Some(last) if last.style == st => last.content.to_mut().push_str(s),
        _ => out.push(Span::styled(s.to_string(), st)),
    };
    let (mut col, mut done) = (0, false);
    for span in &line {
        for g in span.content.graphemes(true) {
            let gw = grapheme_width(g);
            if col + gw <= at || col >= end {
                put(&mut out, g, span.style);
            } else {
                if col < at {
                    put(&mut out, &" ".repeat(at - col), span.style);
                }
                if !std::mem::replace(&mut done, true) {
                    put(&mut out, text, style);
                }
                if col + gw > end {
                    put(&mut out, &" ".repeat(col + gw - end), span.style);
                }
            }
            col += gw;
        }
    }
    if !done {
        put(&mut out, &" ".repeat(at.saturating_sub(col)), Style::default());
        put(&mut out, text, style);
    }
    out
}

/// Piece `p` as text in its styles: most pieces one run; a link whose label hides where it
/// goes, and an app named, a faint run after it.
fn runs_of(app: &App, ws: &WorkspaceId, p: &Piece) -> Vec<(String, Style)> {
    let t = &app.theme;
    let text = plain(p, &|p| app.model.named(ws, p));
    match p {
        Piece::Text(_, marks) => vec![(text, marked(t.text(), *marks))],
        Piece::Code(code) if t.code_marks() => vec![(format!("`{code}`"), t.code())],
        Piece::Code(_) => vec![(text, t.code())],
        Piece::User { id, marks, .. } => {
            let me = app.model.me(ws).is_some_and(|me| me.as_str() == id);
            let mut out = vec![(text, marked(t.mention(me), *marks))];
            let user = app.model.named(ws, p).and_then(|_| app.model.user(ws, &slakio_core::model::UserId::new(id)));
            if user.is_some_and(|u| u.bot) {
                out.push((format!(" {}", app.i18n.label(slakio_core::i18n::Label::MessageApp)), t.faint()));
            }
            out
        }
        Piece::Channel { marks, .. } => vec![(text, marked(t.mention(false), *marks))],
        Piece::Broadcast(_) => vec![(text, t.mention(true))],
        Piece::Group(_) => vec![(text, t.mention(false))],
        // Only a link that opens looks like one; a link that runs code says nowhere it goes.
        Piece::Link { url, label, marks } => {
            let real = label.as_deref().and_then(|l| real_target(url, l));
            let shown = label.clone().filter(|_| real.is_some()).unwrap_or(text);
            let style = if crate::opener::check(url).is_some() { t.markup_link() } else { t.text() };
            let mut out = vec![(shown, marked(style, *marks))];
            out.extend(real.map(|r| (format!(" → {r}"), t.faint())));
            out
        }
    }
}

/// `base` with `marks`.
fn marked(base: Style, marks: Marks) -> Style {
    let mut s = base;
    if marks.bold {
        s = s.add_modifier(Modifier::BOLD);
    }
    if marks.italic {
        s = s.add_modifier(Modifier::ITALIC);
    }
    if marks.strike {
        s = s.add_modifier(Modifier::CROSSED_OUT);
    }
    s
}
