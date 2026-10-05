//! The work area: the main pane with the conversation the list opened and, beside it, the auto
//! thread panel. Each pane is titled with the workspace stripe and the conversation's name,
//! shows its messages ([`super::timeline`]) and, below a divider joined to its border, its own
//! composer: a prompt and the text, no box of its own.
//!
//! ```text
//! ╭ ▌#backend ─────────────────────╮╭ ▌⤷ Thread · #backend ──╮
//! │ Kim   Starting deploy   10:02  ││ Kim   Starting …       │
//! ├─ Message #backend ─────────────┤├─ Reply ────────────────┤
//! │ › Press i to write             ││ ›                      │
//! ╰────────────────────────────────╯╰────────────────────────╯
//! ```

use super::{frame, panel, timeline};
use crate::action::{Action, PaneAction};
use crate::app::App;
use crate::app::pane::Pane;
use crate::app::shell::Region;
use crate::app::work::Side;
use crate::keymap::Ctx;
use crate::screen;
use crate::text::{clip, width};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::Conversation;

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect) {
    let Some(main) = app.work.main.as_ref() else {
        f.render_widget(frame(app, Region::Work, ""), area);
        super::empty::work(f, app, screen::inner(area));
        return;
    };
    let (m, thread) = app.panes();
    if let Some(r) = m {
        draw_pane(f, app, main, r, Side::Main);
    } else {
        main.hits.borrow_mut().clear();
    }
    if let (Some(p), Some(r)) = (app.work.thread.as_ref(), thread) {
        draw_pane(f, app, p, r, Side::Thread);
    } else if let Some(p) = app.work.thread.as_ref() {
        p.hits.borrow_mut().clear();
    }
}

fn draw_pane(f: &mut Frame, app: &App, pane: &Pane, area: Rect, side: Side) {
    let t = &app.theme;
    let focused = app.shell.focus == Region::Work && app.work.side == side;
    let conversation = app.model.target(&pane.target);
    let name = conversation.map(breadcrumb).unwrap_or_default();
    let stripe = app
        .model
        .workspace(pane.target.workspace())
        .map(|w| Span::styled("▌", t.workspace(w.color)))
        .unwrap_or_default();
    let text = match side {
        Side::Main => format!("{name} "),
        Side::Thread => format!("⤷ {} · {name} ", app.i18n.label(Label::PaneThread)),
    };
    let title = Line::from(vec![Span::raw(" "), stripe, Span::styled(text, t.title(focused))]);
    f.render_widget(panel(app, focused, title), area);
    let insert = focused && app.work.insert;
    let view = pane.composer.view(screen::composer_width(area));
    let parts = screen::pane_parts(area, view.lines.len());
    timeline::draw(f, app, pane, parts.messages, focused);
    let Some(divider) = parts.divider else { return };
    // The divider is joined to the pane's border: `├─ Message #backend ───┤`.
    let label = match side {
        Side::Main => app.i18n.msg(&Msg::ComposerMessage { name }).to_string(),
        Side::Thread => app.i18n.label(Label::ComposerReply).to_string(),
    };
    let w = usize::from(area.width);
    let label = clip(&format!(" {label} "), w.saturating_sub(5));
    let rule = "─".repeat(w.saturating_sub(3 + width(&label)));
    let line = Line::from(vec![
        Span::styled("├─", t.border(focused)),
        Span::styled(label, if insert { t.bold() } else { t.muted() }),
        Span::styled(rule, t.border(insert)),
        Span::styled("┤", t.border(focused)),
    ]);
    f.render_widget(Paragraph::new(line), Rect { y: divider, height: 1, ..area });
    let input = parts.input;
    let prompt = Rect { x: input.x + 1, width: 2.min(input.width), ..input };
    f.render_widget(Paragraph::new("›").style(if insert { t.accent() } else { t.faint() }), prompt);
    let text_area = Rect { x: input.x + 3, width: input.width.saturating_sub(4), ..input };
    if pane.composer.is_empty() && !insert {
        let insert_key = super::empty::key_of(app, Action::Pane(PaneAction::Insert), Ctx::PaneNormal);
        let hint = insert_key
            .map(|keys| match side {
                Side::Main => app.i18n.msg(&Msg::ComposerHint { keys }).to_string(),
                Side::Thread => app.i18n.msg(&Msg::ComposerReplyHint { keys }).to_string(),
            })
            .unwrap_or_default();
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

/// How a conversation is named in titles and the status line: `#backend`, `@Minsu`.
pub(super) fn breadcrumb(c: &Conversation) -> String {
    let name = c.name.line();
    if c.is_dm() { format!("@{name}") } else { format!("#{name}") }
}
