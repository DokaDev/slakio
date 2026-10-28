//! A message's text as drawn: its lines of Slack markup ([`crate::mrkdwn`]) as rows of styled
//! spans at a width. Text wraps at blanks; a quoted line wraps under a bar; a code block's line is
//! never wrapped, it is cut with `…`. People and channels are named from the read model when it
//! knows them (their names may come after the message), else by the label Slack wrote, else by
//! their id. The user named, and everyone (`@here`), stand out as GUI Slack marks a mention; an
//! app named is marked. Links are underlined; where a link's label reads as another address
//! than its own, the real target follows it in a faint span of its own (`→ host`), a style no
//! sender's markup gives. A link that runs code is plain text. Inline code is tinted, or keeps
//! its backticks where nothing can tint it; a code block's lines are tinted, or marked by a bar.

use crate::app::App;
use crate::app::pane::Shown;
use crate::mrkdwn::{Block, Marks, Piece, plain, real_target, runs_code};
use crate::text::{clip, wrap_styled};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use slakio_core::model::WorkspaceId;

/// The bar before a quoted line, and the blank after it.
const QUOTE: &str = "▎ ";

/// The bar before a code block's line where nothing tints it (no color), and the blank after it.
const CODE: &str = "│ ";

/// The rows of `m`'s text in a pane of workspace `ws`, `w` cells wide, at most `max` of them;
/// whether text was left out.
pub fn rows(app: &App, ws: &WorkspaceId, m: &Shown, w: usize, max: usize) -> (Vec<Vec<Span<'static>>>, bool) {
    let t = &app.theme;
    let mut out: Vec<Vec<Span<'static>>> = Vec::new();
    for (k, block) in m.body.iter().enumerate() {
        let room = max.saturating_sub(out.len());
        if room == 0 {
            return (out, true);
        }
        match block {
            // Without color nothing tints a code block: a bar marks its lines.
            Block::Code(line) if t.kind == crate::theme::Kind::NoColor => {
                out.push(vec![Span::styled(CODE, t.faint()), Span::styled(clip(line, w.saturating_sub(2)), t.code())]);
            }
            Block::Code(line) => out.push(vec![Span::styled(clip(line, w), t.code())]),
            Block::Line { quote, pieces } => {
                let runs: Vec<(String, Style)> = pieces.iter().flat_map(|p| runs(app, ws, p)).collect();
                let inner = if *quote { w.saturating_sub(2) } else { w };
                let (lines, more) = wrap_styled(&runs, inner, room, t.text());
                for mut line in lines {
                    if *quote {
                        line.insert(0, Span::styled(QUOTE, t.faint()));
                    }
                    out.push(line);
                }
                if more {
                    return (out, true);
                }
            }
        }
        if out.len() >= max && k + 1 < m.body.len() {
            return (out, true);
        }
    }
    (out, false)
}

/// Piece `p` as text in its styles: most pieces one run; a link whose label hides where it
/// goes, and an app named, a faint run after it.
fn runs(app: &App, ws: &WorkspaceId, p: &Piece) -> Vec<(String, Style)> {
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
        Piece::Link { url, marks, .. } if runs_code(url) => vec![(text, marked(t.text(), *marks))],
        Piece::Link { url, label, marks } => {
            let real = label.as_deref().and_then(|l| real_target(url, l));
            let shown = label.clone().filter(|_| real.is_some()).unwrap_or(text);
            let mut out = vec![(shown, marked(t.markup_link(), *marks))];
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
