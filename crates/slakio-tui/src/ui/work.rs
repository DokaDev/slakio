//! The work area: the panes of the tab shown — the conversation the list opened, the auto
//! thread panel beside it, the panes split off. Each pane is titled with the workspace stripe
//! and the conversation's name (a zoomed tab's one pane says `[zoomed]`), shows its messages
//! ([`super::timeline`]) and, below a divider joined to its border, its own composer: a prompt
//! and the text, no box of its own. A pane that holds its view while messages come live below
//! it says how many at its bottom edge (`↓ 3 new`), until the view reaches the newest.
//!
//! ```text
//! ╭ ▌#backend ─────────────────────╮╭ ▌⤷ Thread · #backend ──╮   a DM: ▌MK @Minsu Kim ● active
//! │ MK Minsu Kim  Deploying  10:02 ││ MK Minsu Kim · 10:02   │
//! ├─ Message #backend ─────────────┤├─ Reply ────────────────┤
//! │ › Press i to write             ││ ›                      │
//! ╰────────────────────────────────╯╰────────────────────────╯
//! ```

use super::{avatar_chip, frame, panel, presence_mark, timeline};
use crate::action::{Action, PaneAction};
use crate::app::composer::View;
use crate::app::pane::Pane;
use crate::app::shell::Region;
use crate::app::{App, Focus, PaneHandle};
use crate::keymap::Ctx;
use crate::screen::{self, FrameLayout, PaneLayout};
use crate::text::{clip, width};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::i18n::{Label, Msg};
use slakio_core::layout::PaneId;
use slakio_core::model::{ConversationKind, Presence, Target, Ts};
use std::time::Instant;

pub(super) fn draw(f: &mut Frame, app: &App, layout: &FrameLayout, views: &[(PaneId, View)], now: Instant) {
    if app.work.ids().is_empty() {
        let area = layout.areas.work;
        f.render_widget(frame(app, Region::Work, ""), area);
        super::empty::work(f, app, screen::inner(area));
        return;
    }
    // The pane with the keyboard last: the borders it shares are drawn in its colour. The
    // titles of the others stay: a pane's title is on the border it shares with the pane
    // above, kept as drawn and put back once every border is.
    let mut ids = app.work.ids();
    if let Some(at) = ids.iter().position(|id| Some(*id) == app.work.active()) {
        let active = ids.remove(at);
        ids.push(active);
    }
    let (mut titles, mut spans) = (Vec::new(), Vec::new());
    for id in ids {
        let Some(pane) = app.work.pane(id) else { continue };
        let view = views.iter().find(|(v, _)| *v == id).map(|(_, v)| v);
        match (layout.pane(id), view) {
            (Some(l), Some(view)) => {
                let w = draw_pane(f, app, pane, (l, view), now);
                let (x, y) = (l.rect.x + 1, l.rect.y);
                spans.push((y, x, x + w));
                if Some(id) != app.work.active() {
                    let cells: Vec<_> = (x..x + w).filter_map(|x| f.buffer_mut().cell((x, y)).cloned()).collect();
                    titles.push(((x, y), cells));
                }
            }
            _ => pane.hits.borrow_mut().clear(),
        }
    }
    for ((x, y), cells) in titles {
        for (k, cell) in cells.into_iter().enumerate() {
            if let Some(c) = f.buffer_mut().cell_mut((x + k as u16, y)) {
                *c = cell;
            }
        }
    }
    if layout.panes.len() > 1 {
        join(f, layout, &spans);
    }
}

/// Where the borders of panes side by side meet, the cells that join them: a corner, a tee or a
/// cross, as the lines through each corner of a pane and each end of its composer's divider run.
/// A cell of a title (`titles`: row, first column, column past the end) keeps its text: where a
/// border meets one from across the seam, the title wins.
fn join(f: &mut Frame, layout: &FrameLayout, titles: &[(u16, u16, u16)]) {
    let (mut cols, mut rows) = (Vec::new(), Vec::new());
    let mut points = Vec::new();
    for p in layout.panes.iter().filter(|p| p.rect.width > 1 && p.rect.height > 1) {
        let r = p.rect;
        let (l, t, rt, b) = (r.x, r.y, r.right() - 1, r.bottom() - 1);
        cols.extend([(l, t, b), (rt, t, b)]);
        rows.extend([(t, l, rt), (b, l, rt)]);
        points.extend([(l, t), (rt, t), (l, b), (rt, b)]);
        if let Some(y) = p.parts.divider {
            rows.push((y, l, rt));
            points.extend([(l, y), (rt, y)]);
        }
    }
    for (x, y) in points {
        if titles.iter().any(|&(r, a, b)| r == y && a <= x && x < b) {
            continue;
        }
        let up = cols.iter().any(|&(c, t, b)| c == x && t < y && y <= b);
        let down = cols.iter().any(|&(c, t, b)| c == x && t <= y && y < b);
        let left = rows.iter().any(|&(r, l, rt)| r == y && l < x && x <= rt);
        let right = rows.iter().any(|&(r, l, rt)| r == y && l <= x && x < rt);
        let glyph = match (up, down, left, right) {
            (true, true, true, true) => "┼",
            (true, true, false, true) => "├",
            (true, true, true, false) => "┤",
            (false, true, true, true) => "┬",
            (true, false, true, true) => "┴",
            (false, true, false, true) => "╭",
            (false, true, true, false) => "╮",
            (true, false, false, true) => "╰",
            (true, false, true, false) => "╯",
            (true, true, false, false) => "│",
            _ => "─",
        };
        if let Some(cell) = f.buffer_mut().cell_mut((x, y)) {
            cell.set_symbol(glyph);
        }
    }
}

/// Draw `pane`; the width of its title (on its top border, after the corner).
fn draw_pane(f: &mut Frame, app: &App, pane: &Pane, (layout, view): (&PaneLayout, &View), now: Instant) -> u16 {
    let area = layout.rect;
    let t = &app.theme;
    let focused = app.focus() == Focus::Pane(PaneHandle::of(layout.id));
    let thread = pane.is_thread();
    let conversation = app.model.target(&pane.target);
    let name = conversation.map(breadcrumb).unwrap_or_default();
    let stripe = app
        .model
        .workspace(pane.target.workspace())
        .map(|w| Span::styled("▌", t.workspace(w.color)))
        .unwrap_or_default();
    let text = if thread { format!("⤷ {} · {name} ", thread_name(app, pane)) } else { format!("{name} ") };
    let peer = match conversation.map(|c| &c.kind) {
        Some(ConversationKind::Dm { user }) if !thread => app.model.user(pane.target.workspace(), user),
        _ => None,
    };
    let mut spans = vec![Span::raw(" "), stripe];
    // A DM's title starts with its peer's avatar chip: `MK @Minsu Kim`.
    if let Some(chip) = peer.and_then(|u| avatar_chip(app, &u.workspace, &u.id, u.display_name.line().as_str())) {
        spans.extend([chip, Span::raw(" ")]);
    }
    spans.push(Span::styled(text, t.title(focused)));
    // A zoomed tab shows its active pane alone, and says so.
    if app.work.zoomed() {
        spans.push(Span::styled(format!("[{}] ", app.i18n.label(Label::PaneZoomed)), t.accent()));
    }
    // A DM's title says whether its peer is around: `@Minsu Kim ● active`.
    if let Some((mark, p)) = peer.and_then(|u| presence_mark(u.presence)) {
        let label = match p {
            Presence::Active => Label::PresenceActive,
            Presence::Away => Label::PresenceAway,
            _ => Label::PresenceDnd,
        };
        spans.push(Span::styled(mark, t.presence(p)));
        spans.push(Span::styled(format!(" {} ", app.i18n.label(label)), t.muted()));
    }
    // Cut as the composer's title is: with `…`, two cells of the border left before its corner.
    let title = Line::from(fit(spans, usize::from(area.width).saturating_sub(4)));
    let title_w = title.width() as u16;
    f.render_widget(panel(app, focused, title), area);
    let insert = focused && pane.insert;
    let before = pane.links.borrow().clone();
    timeline::draw(f, app, pane, layout.parts.messages, focused);
    if *pane.links.borrow() != before {
        pane.links_moved.set(Some(now));
    }
    if !draw_divider(f, app, pane, (area, layout.parts.divider), (name, focused)) {
        return title_w;
    }
    let input = layout.parts.input;
    let prompt = Rect { x: input.x + 1, width: 2.min(input.width), ..input };
    f.render_widget(Paragraph::new("›").style(if insert { t.accent() } else { t.faint() }), prompt);
    let text_area = Rect { x: input.x + 3, width: input.width.saturating_sub(4), ..input };
    if app.work.draft(pane).is_empty() && !insert {
        let hint = composer_hint(app, thread);
        f.render_widget(Paragraph::new(clip(&hint, usize::from(text_area.width))).style(t.faint()), text_area);
        return title_w;
    }
    let h = usize::from(text_area.height);
    let top = (view.cursor.0 + 1).saturating_sub(h);
    for (k, l) in view.lines.iter().skip(top).take(h).enumerate() {
        let row = Rect { y: text_area.y + k as u16, height: 1, ..text_area };
        f.render_widget(Paragraph::new(l.as_str()).style(t.text()), row);
    }
    if insert {
        let x = text_area.x + (view.cursor.1 as u16).min(text_area.width.saturating_sub(1));
        let y = text_area.y + (view.cursor.0 - top) as u16;
        f.set_cursor_position(Position { x, y });
    }
    // Width of what is drawn never exceeds the area: the composer wraps at its width.
    debug_assert!(view.lines.iter().all(|l| width(l) <= usize::from(text_area.width.max(2))));
    title_w
}

pub(super) use crate::app::model::breadcrumb;

/// What a thread pane is called: `Thread`; beside another pane of the tab on a thread of the same
/// conversation, its first message's author (`⤷ Ivy Moon · #long-threads`), or where that is the
/// other's too, the start of its text.
fn thread_name(app: &App, pane: &Pane) -> String {
    let first = |p: &Pane| {
        let Target::Thread { thread, .. } = p.target else { return None };
        app.work.timeline(p).items.first().filter(|m| m.ts == thread).cloned()
    };
    let others: Vec<_> = app
        .work
        .ids()
        .into_iter()
        .filter_map(|id| app.work.pane(id))
        .filter(|p| !std::ptr::eq(*p, pane) && p.is_thread() && p.target.conversation() == pane.target.conversation())
        .filter(|p| p.target.workspace() == pane.target.workspace())
        .collect();
    let (true, Some(m)) = (!others.is_empty(), first(pane)) else {
        return app.i18n.label(Label::PaneThread).to_string();
    };
    let same = others.iter().any(|p| first(p).is_some_and(|o| o.author == m.author));
    if same {
        let text = m.text.as_str().split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.is_empty() {
            return clip(&text, 20);
        }
    }
    m.author.as_str().to_string()
}

/// `spans` in `w` cells: the first that does not fit whole is cut with `…` (a blank at its end
/// goes first), and those after it are left out.
fn fit(spans: Vec<Span<'_>>, w: usize) -> Vec<Span<'_>> {
    let mut out = Vec::new();
    let mut used = 0;
    for s in spans {
        let room = w.saturating_sub(used);
        if width(&s.content) <= room {
            used += width(&s.content);
            out.push(s);
            continue;
        }
        // Words that would just fill the room touch the border's dashes: cut, they end in `…`.
        let trimmed = s.content.trim_end();
        let cut = clip(trimmed, if width(trimmed) == room { room.saturating_sub(1) } else { room });
        if !cut.is_empty() {
            out.push(Span::styled(cut, s.style));
        }
        break;
    }
    out
}

/// The divider over `pane`'s composer, joined to its border (`├─ Message #backend ───┤`), and the
/// count of messages that came live below a view the user holds, said faintly at the pane's
/// bottom edge: on the divider, or on the border where there is none (then `false`).
fn draw_divider(
    f: &mut Frame,
    app: &App,
    pane: &Pane,
    (area, divider): (Rect, Option<u16>),
    (name, focused): (String, bool),
) -> bool {
    let t = &app.theme;
    let new = new_below(app, pane);
    let new = (new > 0).then(|| format!(" {} ", app.i18n.msg(&Msg::PaneNewBelow { count: new as u64 })));
    let Some(divider) = divider else {
        if let Some(n) = new.filter(|n| width(n) + 4 <= usize::from(area.width)) {
            let nw = width(&n) as u16;
            let at = Rect::new(area.right() - 2 - nw, area.bottom() - 1, nw, 1);
            f.render_widget(Paragraph::new(Span::styled(n, t.faint())), at);
        }
        return false;
    };
    let label = if pane.is_thread() {
        app.i18n.label(Label::ComposerReply).to_string()
    } else {
        app.i18n.msg(&Msg::ComposerMessage { name }).to_string()
    };
    let w = usize::from(area.width);
    let label = clip(&format!(" {label} "), w.saturating_sub(5));
    let new = new.filter(|n| 3 + width(&label) + width(n) + 2 <= w);
    let rule = "─".repeat(w.saturating_sub(3 + width(&label) + new.as_deref().map_or(0, |n| width(n) + 1)));
    // One color, the pane's border's, so the tees join it.
    let style = t.border(focused);
    let mut spans = vec![Span::styled("├─", style), Span::styled(label, style), Span::styled(rule, style)];
    if let Some(n) = new {
        spans.extend([Span::styled(n, t.faint()), Span::styled("─", style)]);
    }
    spans.push(Span::styled("┤", style));
    f.render_widget(Paragraph::new(Line::from(spans)), Rect { y: divider, height: 1, ..area });
    true
}

/// How many messages came below the view `pane` holds since it held it: live, or spliced in by a
/// catch-up.
fn new_below(app: &App, pane: &Pane) -> usize {
    let (Some(since), Some(bottom)) = (pane.fresh.get(), pane.bottom.get()) else { return 0 };
    let (items, seen) = (&app.work.timeline(pane).items, pane.seen.borrow());
    let new = |ts: Ts| ts > since || seen.binary_search(&ts).is_err();
    items.iter().rev().take_while(|m| m.ts > bottom).filter(|m| new(m.ts)).count()
}

/// What an empty composer says: how to write (or reply), or, where nothing can be sent, that.
fn composer_hint(app: &App, thread: bool) -> String {
    if app.read_only() {
        return app.i18n.label(Label::ComposerReadOnly).to_string();
    }
    let insert_key = super::empty::key_of(app, Action::Pane(PaneAction::Insert), Ctx::PaneNormal);
    insert_key
        .map(|keys| {
            let hint = if thread { Msg::ComposerReplyHint { keys } } else { Msg::ComposerHint { keys } };
            app.i18n.msg(&hint).to_string()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
