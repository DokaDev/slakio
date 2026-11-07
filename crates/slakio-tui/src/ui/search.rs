//! The search popup over the dimmed screen, in the palette's style: the workspace searched in
//! the title, the words on the first line (`/` before them), a rule, then the results — each
//! its conversation (accent), its author and its time (muted, on the right), and under them the
//! text with the words found underlined — the one under the cursor on the selection bar. How
//! many were found, which page, goes on the bottom border, the keys on its right. Every remote
//! text is sanitised to one line before it is drawn.
//!
//! ```text
//! ╭ Search · acme.slack.com ───────────────────────────────────────╮
//! │ / deploy in:#backend                                            │
//! ├─────────────────────────────────────────────────────────────────┤
//! │ #backend  Minsu Kim                            2026-11-03 14:02 │
//! │   the deploy went out at three                                  │
//! ╰ 134 found · 1/7 ──────── ↑↓ select · Enter open · Tab workspace ╯
//! ```

use crate::app::App;
use crate::app::model::breadcrumb;
use crate::app::search::{HIT_LINES, Showing, runs};
use crate::text::{clip, width, wrap};
use crate::theme::Selection;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use slakio_core::backend::Hit;
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::WorkspaceId;
use slakio_core::sanitize::Remote;

pub(super) fn draw(f: &mut Frame, app: &App) {
    let t = &app.theme;
    let Some(b) = app.search_box() else { return };
    f.render_widget(Clear, b.rect);
    f.buffer_mut().set_style(b.rect, t.surface());
    let w = usize::from(b.rect.width);
    let ws = app.search_scope().cloned();
    let place = ws.as_ref().map(|ws| app.workspace_label(ws)).unwrap_or_default();
    let title = app.i18n.msg(&Msg::SearchTitle { workspace: place });
    let title = clip(&format!(" {title} "), w.saturating_sub(4));
    let mut block = Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(t.border(true))
        .title(Line::from(Span::styled(title, t.bold())));
    let count = count(app);
    let keys = format!(" {} ", app.i18n.label(Label::SearchKeys));
    if width(&count) + width(&keys) + 6 <= w {
        block = block.title_bottom(Line::from(Span::styled(keys, t.muted())).right_aligned());
    }
    if !count.is_empty() && width(&count) + 4 <= w {
        block = block.title_bottom(Line::from(Span::styled(count, t.muted())).left_aligned());
    }
    f.render_widget(block, b.rect);

    // The words: `/` and what was typed, or what can be typed.
    let input = b.input;
    f.render_widget(Paragraph::new("/").style(t.key()), Rect { x: input.x + 1, width: 1, ..input });
    let area = Rect { x: input.x + 3, width: input.width.saturating_sub(4), ..input };
    let typed = app.search_query();
    let shown = tail(typed, usize::from(area.width.saturating_sub(1)));
    if typed.is_empty() {
        let hint = clip(&app.i18n.label(Label::SearchPlaceholder), usize::from(area.width));
        f.render_widget(Paragraph::new(hint).style(t.faint()), area);
    } else {
        f.render_widget(Paragraph::new(shown.clone()).style(t.text()), area);
    }
    let cx = area.x + width(&shown) as u16;
    if app.searching() && !app.cmdline.is_open() {
        f.set_cursor_position(Position { x: cx.min(area.right().saturating_sub(1)), y: input.y });
    }
    let rule = format!("├{}┤", "─".repeat(usize::from(input.width)));
    f.render_widget(
        Paragraph::new(rule).style(t.border(true)),
        Rect { x: b.rect.x, y: input.y + 1, width: b.rect.width, height: 1 },
    );

    let list = b.list;
    let said = |label: Label| app.i18n.label(label).to_string();
    let (words, style) = match app.search_showing() {
        Showing::Results => return draw_hits(f, app, ws.as_ref(), b.first, b.shown(), list),
        Showing::Hint => (said(Label::SearchHint), t.faint()),
        Showing::Waiting => (said(Label::SearchWaiting), t.faint()),
        Showing::Nothing => (said(Label::SearchNothing), t.muted()),
        Showing::Failed => {
            let why = app.search_failed_kind().map(|k| said(crate::app::failure_label(k))).unwrap_or_default();
            (format!("{why} {}", said(Label::SearchRetry)), t.warning())
        }
    };
    let iw = usize::from(list.width.saturating_sub(4));
    for (k, line) in wrap(&words, iw, usize::from(list.height)).0.into_iter().enumerate() {
        let row = Rect { x: list.x + 2, y: list.y + k as u16, width: list.width.saturating_sub(4), height: 1 };
        f.render_widget(Paragraph::new(line).style(style), row);
    }
}

/// How many were found and which page, or that a search is on its way.
fn count(app: &App) -> String {
    let waiting = app.search_waiting().then(|| app.i18n.label(Label::SearchWaiting).to_string());
    let page = app.search_found().filter(|f| f.total > 0).map(|f| {
        app.i18n
            .msg(&Msg::SearchCount { total: f.total.to_string(), page: f.page.to_string(), pages: f.pages.to_string() })
            .to_string()
    });
    match (page, waiting) {
        (Some(p), Some(w)) => format!(" {p} · {w} "),
        (Some(p), None) => format!(" {p} "),
        (None, Some(w)) => format!(" {w} "),
        (None, None) => String::new(),
    }
}

fn draw_hits(f: &mut Frame, app: &App, ws: Option<&WorkspaceId>, first: usize, shown: usize, list: Rect) {
    let t = &app.theme;
    let Some(found) = app.search_found() else { return };
    let iw = usize::from(list.width.saturating_sub(4));
    for (k, (i, hit)) in found.hits.iter().enumerate().skip(first).take(shown).enumerate() {
        let y = list.y + k as u16 * HIT_LINES;
        let top = Rect { x: list.x + 2, y, width: list.width.saturating_sub(4), height: 1 };
        let when = format!("{} {}", crate::time::date(hit.ts), crate::time::hm(hit.ts));
        let where_ = clip(&place_of(app, ws, hit), iw / 2);
        let who_room = iw.saturating_sub(width(&where_) + width(&when) + 4);
        let who = clip(&author_of(app, ws, hit), who_room);
        let line = Line::from(vec![
            Span::styled(where_.clone(), t.accent().add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(who, t.author()),
        ]);
        f.render_widget(Paragraph::new(line), top);
        if width(&when) + width(&where_) + 2 < iw {
            let wx = top.right().saturating_sub(width(&when) as u16);
            f.render_widget(
                Paragraph::new(when.clone()).style(t.muted()),
                Rect { x: wx, width: width(&when) as u16, ..top },
            );
        }
        let text = Rect { x: top.x + 2, y: y + 1, width: top.width.saturating_sub(2), height: 1 };
        f.render_widget(Paragraph::new(snippet(app, ws, hit, usize::from(text.width))), text);
        if i == app.search_selected() {
            let bar = Rect { x: list.x, y, width: list.width, height: HIT_LINES };
            t.paint_selection(f.buffer_mut(), bar, Selection::Focused);
        }
    }
}

/// Where `hit` is: the conversation as the list names it, else as the search named it.
fn place_of(app: &App, ws: Option<&WorkspaceId>, hit: &Hit) -> String {
    let c = app.model.conversations().iter().find(|c| Some(&c.workspace) == ws && c.id == hit.conversation);
    match c {
        Some(c) => breadcrumb(c),
        None if !hit.name.is_empty() => format!("#{}", hit.name.line()),
        None => Remote::new(hit.conversation.as_str()).line().into_string(),
    }
}

/// Who wrote `hit`: the name the app knows, else the one the search gave.
fn author_of(app: &App, ws: Option<&WorkspaceId>, hit: &Hit) -> String {
    let known = ws.and_then(|ws| app.model.user(ws, &hit.user)).map(|u| u.display_name.line().into_string());
    let given = (!hit.author.is_empty()).then(|| hit.author.line().into_string());
    known.filter(|n| !n.is_empty()).or(given).unwrap_or_else(|| Remote::new(hit.user.as_str()).line().into_string())
}

/// `hit`'s text on one line `w` wide, Slack's markup read (people and channels by the names the
/// app knows, links by their text), the words found underlined; when the first of them would be
/// past the first third, the text before it is cut (`…` in its place).
fn snippet(app: &App, ws: Option<&WorkspaceId>, hit: &Hit, w: usize) -> Line<'static> {
    use crate::mrkdwn::{Block, Piece, parse, plain};
    let t = &app.theme;
    let name = |p: &Piece| ws.and_then(|ws| app.model.named(ws, p));
    let flat = |s: String| -> String {
        let lines = parse(Remote::new(s).block().as_str());
        let lines: Vec<String> = (lines.iter())
            .map(|b| match b {
                Block::Line { pieces, .. } => pieces.iter().map(|p| plain(p, &name)).collect(),
                Block::Code(code) => code.clone(),
            })
            .collect();
        Remote::new(lines.join(" ")).line().into_string()
    };
    let mut parts: Vec<(String, bool)> = runs(hit).into_iter().map(|(s, m)| (flat(s), m)).collect();
    let before: usize = parts.iter().take_while(|(_, m)| !m).map(|(s, _)| width(s)).sum();
    let mut spans = Vec::new();
    let mut room = w;
    if before > w / 3 && parts.first().is_some_and(|(_, m)| !m) {
        let keep = w / 6;
        let lead = &parts[0].0;
        let cut: String = tail(lead, keep);
        parts[0].0 = cut;
        spans.push(Span::styled("…", t.faint()));
        room = room.saturating_sub(1);
    }
    for (s, matched) in parts {
        if room == 0 {
            break;
        }
        let s = clip(&s, room);
        room = room.saturating_sub(width(&s));
        let style = if matched { t.key_warm().add_modifier(Modifier::UNDERLINED) } else { t.text() };
        spans.push(Span::styled(s, style));
    }
    Line::from(spans)
}

/// The end of `s` that fits in `w` cells.
fn tail(s: &str, w: usize) -> String {
    if width(s) <= w {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars().rev() {
        let mut next = c.to_string();
        next.push_str(&out);
        if width(&next) > w {
            break;
        }
        out = next;
    }
    out
}
