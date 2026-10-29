//! The work area: the main pane with the conversation the list opened and, beside it, the auto
//! thread panel. Each pane is titled with the workspace stripe and the conversation's name,
//! shows its messages ([`super::timeline`]) and, below a divider joined to its border, its own
//! composer: a prompt and the text, no box of its own. A pane that holds its view while
//! messages come live below it says how many at its bottom edge (`↓ 3 new`), until the view
//! reaches the newest.
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
use slakio_core::model::{ConversationKind, Presence, Ts};

pub(super) fn draw(f: &mut Frame, app: &App, layout: &FrameLayout, views: &[(PaneId, View)]) {
    if app.work.ids().is_empty() {
        let area = layout.areas.work;
        f.render_widget(frame(app, Region::Work, ""), area);
        super::empty::work(f, app, screen::inner(area));
        return;
    }
    for id in app.work.ids() {
        let Some(pane) = app.work.pane(id) else { continue };
        let view = views.iter().find(|(v, _)| *v == id).map(|(_, v)| v);
        match (layout.pane(id), view) {
            (Some(l), Some(view)) => draw_pane(f, app, pane, (l, view)),
            _ => pane.hits.borrow_mut().clear(),
        }
    }
}

fn draw_pane(f: &mut Frame, app: &App, pane: &Pane, (layout, view): (&PaneLayout, &View)) {
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
    let text =
        if thread { format!("⤷ {} · {name} ", app.i18n.label(Label::PaneThread)) } else { format!("{name} ") };
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
    let title = Line::from(spans);
    f.render_widget(panel(app, focused, title), area);
    let insert = focused && pane.insert;
    timeline::draw(f, app, pane, layout.parts.messages, focused);
    if !draw_divider(f, app, pane, (area, layout.parts.divider), (name, focused)) {
        return;
    }
    let input = layout.parts.input;
    let prompt = Rect { x: input.x + 1, width: 2.min(input.width), ..input };
    f.render_widget(Paragraph::new("›").style(if insert { t.accent() } else { t.faint() }), prompt);
    let text_area = Rect { x: input.x + 3, width: input.width.saturating_sub(4), ..input };
    if app.work.draft(pane).is_empty() && !insert {
        let hint = composer_hint(app, thread);
        f.render_widget(Paragraph::new(clip(&hint, usize::from(text_area.width))).style(t.faint()), text_area);
        return;
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
}

pub(super) use crate::app::model::breadcrumb;

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
