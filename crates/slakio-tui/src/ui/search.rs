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
use crate::app::search::{HIT_LINES, Showing};
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
    // The keys beside the count: all of them, else the ones that matter with results there.
    let keys = [Label::SearchKeys, Label::SearchKeysShort, Label::SearchKeysTiny]
        .map(|l| format!(" {} ", app.i18n.label(l)))
        .into_iter()
        .find(|keys| width(&count) + width(keys) + 6 <= w);
    if let Some(keys) = keys {
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
        // Kept from before: shown selected, as typing replaces them.
        let style = if app.search_fresh() { t.text().add_modifier(Modifier::REVERSED) } else { t.text() };
        f.render_widget(Paragraph::new(shown.clone()).style(style), area);
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
            // What is left of Slack's wait says why on its own.
            let words = match app.search_wait() {
                (_, Some(left)) => app.i18n.msg(&Msg::SearchWaitLeft { seconds: left.to_string() }).to_string(),
                (Some(secs), None) => {
                    format!("{why}. {}", app.i18n.msg(&Msg::SearchRetryIn { seconds: secs.to_string() }))
                }
                (None, None) => format!("{why}. {}", said(Label::SearchRetry)),
            };
            (words, t.warning())
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
        let when = stamp(app, hit);
        // A reply is marked as one: `Enter` opens its thread beside the conversation.
        let place =
            if hit.thread.is_some() { format!("{} ⤷", place_of(app, ws, hit)) } else { place_of(app, ws, hit) };
        let where_ = clip(&place, iw / 2);
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
        f.render_widget(Paragraph::new(snippet(app, ws, app.search_runs(i), usize::from(text.width))), text);
        if i == app.search_selected() {
            // The bar is over both lines of the result.
            for row in y..y + HIT_LINES {
                let bar = Rect { x: list.x, y: row, width: list.width, height: 1 };
                t.paint_selection(f.buffer_mut(), bar, Selection::Focused);
            }
        }
    }
}

/// When `hit` was written, as short as is clear: the time today, the month and day with it
/// this year, the date alone before that (all in the zone shown).
fn stamp(app: &App, hit: &Hit) -> String {
    let z = &app.zone;
    let (date, hm) = (crate::time::date(hit.ts, z), crate::time::hm(hit.ts, z));
    let today = crate::time::date(app.now_ts(), z);
    match (date.split_once('-'), today.split_once('-')) {
        _ if date == today => hm,
        (Some((y, md)), Some((ty, _))) if y == ty => format!("{md} {hm}"),
        _ => date,
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

/// A result's text (its `runs`) on one line `w` wide, Slack's markup read (people and channels
/// by the names the app knows, links by their text), the words found underlined; when the first
/// of them would be past the first third, the text before it is cut at a word (`…` in its
/// place). Runs past the end of the line are not read.
fn snippet(app: &App, ws: Option<&WorkspaceId>, runs: &[(String, bool)], w: usize) -> Line<'static> {
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
    let mut spans = Vec::new();
    let mut room = w;
    // Runs alternate: only the first can be plain text before the first word found; with no
    // word found after it (a search of only `in:` or `from:`), the text shows from its start.
    let found = runs.iter().any(|(_, m)| *m);
    let mut lead = runs.first().filter(|(_, m)| !m && found).map(|(s, _)| flat(s.clone()));
    if let Some(l) = lead.as_mut().filter(|l| width(l) > w / 3) {
        let cut = tail(l, w / 6);
        // At a word: what is left of a word cut in two goes, the whole of it when it is all
        // there is.
        let whole = l.len() - cut.len();
        let at_word = l[..whole].ends_with(char::is_whitespace) || cut.starts_with(char::is_whitespace);
        *l = match cut.split_once(char::is_whitespace) {
            _ if at_word => cut.trim_start().to_string(),
            Some((_, rest)) => rest.trim_start().to_string(),
            None => String::new(),
        };
        spans.push(Span::styled("…", t.faint()));
        room = room.saturating_sub(1);
    }
    let skip = usize::from(lead.is_some());
    let rest = runs.iter().skip(skip).map(|(s, m)| (s.clone(), *m));
    for (s, matched) in lead.map(|l| (l, false)).into_iter().chain(rest.map(|(s, m)| (flat(s), m))) {
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
