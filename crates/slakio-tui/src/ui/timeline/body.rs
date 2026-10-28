//! A message's text as drawn: its lines of Slack markup ([`crate::mrkdwn`]) as rows of styled
//! spans at a width. Text wraps at blanks; a quoted line wraps under a bar; a code block's line is
//! never wrapped, it is cut with `…`. People and channels are named from the read model when it
//! knows them (their names may come after the message), else by the label Slack wrote, else by
//! their id. The user named, and everyone (`@here`), stand out as GUI Slack marks a mention.

use crate::app::App;
use crate::app::pane::Shown;
use crate::mrkdwn::{Block, Marks, Piece, plain};
use crate::text::{clip, wrap_styled};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use slakio_core::model::WorkspaceId;

/// The bar before a quoted line, and the blank after it.
const QUOTE: &str = "▎ ";

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
            Block::Code(line) => out.push(vec![Span::styled(clip(line, w), t.code())]),
            Block::Line { quote, pieces } => {
                let runs: Vec<(String, Style)> = pieces.iter().map(|p| run(app, ws, p)).collect();
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

/// Piece `p` as text and its style.
fn run(app: &App, ws: &WorkspaceId, p: &Piece) -> (String, Style) {
    let t = &app.theme;
    let text = plain(p, &|p| app.model.named(ws, p));
    let style = match p {
        Piece::Text(_, marks) => marked(t.text(), *marks),
        Piece::Code(_) => t.code(),
        Piece::User { id, marks, .. } => {
            marked(t.mention(app.model.me(ws).is_some_and(|me| me.as_str() == id)), *marks)
        }
        Piece::Channel { marks, .. } => marked(t.mention(false), *marks),
        Piece::Broadcast(_) => t.mention(true),
        Piece::Group(_) => t.mention(false),
        Piece::Link { marks, .. } => marked(t.link(), *marks),
    };
    (text, style)
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
