//! Activity in the list panel: two rows a notice — its mark, where it is and when; then who
//! and the message's text on one line — unread ones bold (the mark of an unread mention red),
//! read ones muted. With several workspaces each notice starts with its workspace's stripe.
//! Before anything is loaded the list says it is loading, or why it could not be; at the end of
//! the list, a page on its way or one that failed says so.
//!
//! ```text
//! │▌@ #incidents         14:02 │   @ you, ! everyone, & a group, * a word, ⤷ a thread,
//! │▌  Minsu Kim: the deplo…    │   + a reaction, » a DM
//! │▌⤷ #long-thr…   11-05 09:14 │
//! │▌  Jiho Park: looks good    │
//! │   Loading…                 │   a page on its way
//! ```
//!
//! Every remote text is sanitised to one line before it is drawn.

use crate::app::App;
use crate::app::activity::Foot;
use crate::app::model::breadcrumb;
use crate::mrkdwn::{Block, Piece, parse, plain};
use crate::text::{clip, width};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::backend::{Notice, NoticeKind};
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::{Target, WorkspaceId};
use slakio_core::sanitize::Remote;

/// The mark of a notice of `kind`.
pub fn mark(kind: NoticeKind) -> &'static str {
    match kind {
        NoticeKind::Mention => "@",
        NoticeKind::Everyone => "!",
        NoticeKind::Group => "&",
        NoticeKind::Keyword => "*",
        NoticeKind::Thread => "⤷",
        NoticeKind::Reaction => "+",
        NoticeKind::Dm => "»",
    }
}

/// Draw `spans` from column `x` of `row`, at most `w` cells.
fn put(f: &mut Frame, row: Rect, x: u16, w: u16, spans: Vec<Span<'_>>) {
    let x = row.x + x;
    if x >= row.right() || w == 0 {
        return;
    }
    let w = w.min(row.right() - x);
    f.render_widget(Paragraph::new(Line::from(spans)), Rect { x, width: w, ..row });
}

/// The workspace's stripe in the gutter, when several are signed in to.
fn stripe(f: &mut Frame, app: &App, ws: &WorkspaceId, area: Rect) {
    if app.model.workspaces().len() < 2 {
        return;
    }
    if let Some(w) = app.model.workspace(ws) {
        put(f, area, 0, 1, vec![Span::styled("▌", app.theme.workspace(w.color))]);
    }
}

/// A notice's first row: its mark, its conversation, its time at the right.
pub(super) fn notice(f: &mut Frame, app: &App, i: usize, area: Rect) {
    let t = &app.theme;
    let Some((ws, n)) = app.model.notices.get(i) else { return };
    stripe(f, app, ws, area);
    let mark_style = match (n.unread, n.kind.is_mention()) {
        (true, mention) => t.dot(mention),
        (false, _) => t.faint(),
    };
    put(f, area, 1, 1, vec![Span::styled(mark(n.kind), mark_style)]);
    let when = super::search::short_time(app, n.at);
    let w = width(&when) as u16;
    let time_x = area.width.saturating_sub(w + 1);
    let room = time_x.saturating_sub(3 + 1);
    let fits = w + 6 <= area.width;
    let room = if fits { room } else { area.width.saturating_sub(4) };
    let place = clip(&place(app, ws, n), usize::from(room));
    let style = if n.unread { t.bold() } else { t.text() };
    put(f, area, 3, room, vec![Span::styled(place, style)]);
    if fits {
        put(f, area, time_x, w, vec![Span::styled(when, t.muted())]);
    }
}

/// A notice's second row: who wrote it (or reacted), and the message's text.
pub(super) fn meta(f: &mut Frame, app: &App, i: usize, area: Rect) {
    let t = &app.theme;
    let Some((ws, n)) = app.model.notices.get(i) else { return };
    stripe(f, app, ws, area);
    let room = area.width.saturating_sub(3 + 1);
    let mut left = usize::from(room);
    let mut spans = Vec::new();
    let who = author(app, ws, n);
    let lead = match &n.reaction {
        Some(emoji) => {
            let emoji = emoji.line().into_string();
            app.i18n.msg(&Msg::ActivityReacted { who, emoji }).to_string()
        }
        None if who.is_empty() => String::new(),
        None => format!("{who}:"),
    };
    let author_style = if n.unread { t.author() } else { t.muted() };
    if !lead.is_empty() {
        let lead = clip(&lead, left);
        left = left.saturating_sub(width(&lead) + 1);
        spans.push(Span::styled(lead, author_style));
        spans.push(Span::raw(" "));
    }
    let (text, style): (String, Style) = match &n.text {
        Some(text) => (flat(app, ws, text), if n.unread { t.text() } else { t.muted() }),
        None => (app.i18n.label(Label::ActivityNoText).to_string(), t.faint()),
    };
    spans.push(Span::styled(clip(text.trim(), left), style));
    put(f, area, 3, room, spans);
}

/// What the end of the list says (a page on its way, one that failed), in `area`.
pub(super) fn foot(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let Some(foot) = app.activity_foot() else { return };
    let room = area.width.saturating_sub(4);
    let spans = match foot {
        Foot::Loading => vec![Span::styled(app.i18n.label(Label::ListLoading).to_string(), t.faint())],
        Foot::Failed(kind) => {
            let mut s = vec![Span::styled(app.i18n.label(Label::ActivityMoreFailed).to_string(), t.muted())];
            if let Some(keys) = super::retry_key(app).filter(|_| kind.retryable()) {
                let hint = app.i18n.msg(&Msg::RetryHint { keys }).to_string();
                s.push(Span::styled(format!(" · {hint}"), t.muted()));
            }
            s
        }
    };
    let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
    let style = spans.first().map_or(t.muted(), |s| s.style);
    put(f, area, 3, room, vec![Span::styled(clip(&text, usize::from(room)), style)]);
}

/// Where `n` is: the conversation as the list names it, else its id.
fn place(app: &App, ws: &WorkspaceId, n: &Notice) -> String {
    let target = Target::Conversation { workspace: ws.clone(), conversation: n.conversation.clone() };
    match app.model.target(&target) {
        Some(c) => breadcrumb(c),
        None => Remote::new(n.conversation.as_str()).line().into_string(),
    }
}

/// Who wrote `n`, or reacted: by the name the app knows, else their id; nothing when unknown.
fn author(app: &App, ws: &WorkspaceId, n: &Notice) -> String {
    if n.user.as_str().is_empty() {
        return String::new();
    }
    let known = app.model.user(ws, &n.user).map(|u| u.display_name.line().into_string());
    known.filter(|n| !n.is_empty()).unwrap_or_else(|| Remote::new(n.user.as_str()).line().into_string())
}

/// `text` on one line: Slack's markup read (people and channels by the names the app knows,
/// links by their text), sanitised.
fn flat(app: &App, ws: &WorkspaceId, text: &Remote) -> String {
    let name = |p: &Piece| app.model.named(ws, p);
    let lines: Vec<String> = (parse(text.block().as_str()).iter())
        .map(|b| match b {
            Block::Line { pieces, .. } => pieces.iter().map(|p| plain(p, &name)).collect(),
            Block::Code(code) => code.clone(),
        })
        .collect();
    Remote::new(lines.join(" ")).line().into_string()
}
