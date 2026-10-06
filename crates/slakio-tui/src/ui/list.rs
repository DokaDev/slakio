//! The list panel: the view picked on the rail. Home lists the workspace's sidebar sections
//! (folded with `Enter`) and their conversations, a blank row between two sections; DMs lists
//! the DMs. Views built in a later version say so and how to get back.
//!
//! ```text
//! ╭ Home ─────────────────────╮
//! │ ▾ Favorites               │   col 0 gutter, col 1 fold mark, col 3 the section's name
//! │   # backend               │   col 3 prefix, col 5 name
//! │   # incidents         3   │   a pill: mentions (or unread DM messages)
//! │                           │
//! │ ▸ Ops                 2   │   folded: how many of its conversations are unread
//! │   ▪ secret-proj           │   ▪ = private
//! │   # partner-shared    ext │   ext = shared with another organization
//! │   # feed-customer-1…  1   │   names end in … where they would touch the pill
//! │   ● Minsu Kim             │   a DM: its peer is active (●), away (○) or in do not
//! │   ◐ Jiyoung Lee           │   disturb (◐); `@` while unknown
//! ```
//!
//! Unread conversations are bold, muted ones faint; nothing else marks them.

use super::{count_pill, frame, highlight, presence_mark, view_label};
use crate::app::App;
use crate::app::model::Row;
use crate::app::shell::{Region, View};
use crate::screen;
use crate::text::{clip, width};
use crate::theme::Selection;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use slakio_core::i18n::Label;
use slakio_core::model::{ConversationKind, Presence};

/// Nerd Font glyphs: `nf-md-lock` (a private channel) and `nf-md-link_variant` (a channel shared
/// with another organization).
const ICON_PRIVATE: &str = "\u{F033E}";
const ICON_EXTERNAL: &str = "\u{F0339}";

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let title = app.i18n.label(view_label(app.shell.view));
    f.render_widget(frame(app, Region::List, &title), area);
    let inner = screen::inner(area);
    if inner.is_empty() {
        return;
    }
    let rows = app.shell.rows(&app.model);
    if !app.model.is_loaded() {
        let p = Paragraph::new(app.i18n.label(Label::ListLoading).to_string()).style(t.faint());
        f.render_widget(p, pad(inner));
        return;
    }
    if app.shell.view.is_placeholder() || rows.is_empty() {
        super::empty::list(f, app, inner);
        return;
    }
    let focused = app.shell.focus == Region::List;
    for (k, &row) in rows.iter().enumerate().skip(app.shell.list_top).take(usize::from(inner.height)) {
        let y = inner.y + (k - app.shell.list_top) as u16;
        let area = Rect { y, height: 1, ..inner };
        let mark = draw_row(f, app, row, area);
        if k == app.shell.list_cursor && row.is_selectable() {
            highlight(f, app, area, if focused { Selection::Focused } else { Selection::Unfocused });
            // A presence mark keeps its color on the bar where it reads there.
            if let Some((x, fg)) = mark.and_then(|(x, p)| t.presence_selected(p).map(|fg| (x, fg))) {
                f.buffer_mut()[(x, y)].set_fg(fg);
            }
        }
    }
}

/// Inside the gutter and the right padding.
fn pad(r: Rect) -> Rect {
    Rect { x: r.x + 1, width: r.width.saturating_sub(2), ..r }
}

/// Draw `text` from column `x` of `row`, at most `w` cells.
fn put(f: &mut Frame, row: Rect, x: u16, w: u16, spans: Vec<Span<'_>>) {
    let x = row.x + x;
    if x >= row.right() || w == 0 {
        return;
    }
    let w = w.min(row.right() - x);
    f.render_widget(Paragraph::new(Line::from(spans)), Rect { x, width: w, ..row });
}

/// A pill or mark at the right of `row`, one cell in from its edge; its width.
fn right(f: &mut Frame, row: Rect, text: &str, style: Style) -> u16 {
    let w = width(text) as u16;
    if w == 0 || w + 2 > row.width {
        return 0;
    }
    put(f, row, row.width - 1 - w, w, vec![Span::styled(text.to_string(), style)]);
    w
}

/// Draw `row` in `area`; where its presence mark is (the column), and whose, when it has one
/// that is not faint.
fn draw_row(f: &mut Frame, app: &App, row: Row, area: Rect) -> Option<(u16, Presence)> {
    let t = &app.theme;
    match row {
        Row::Spacer => None,
        Row::Section(i) => {
            let s = app.model.section(i);
            let folded = app.shell.collapsed.contains(&s.id);
            // A folded section tells what it hides: a pill of mentions, else the number of
            // unread conversations.
            let used = if folded {
                match app.model.section_counts(i) {
                    (_, m) if m > 0 => right(f, area, &count_pill(m), t.badge()),
                    (u, _) if u > 0 => right(f, area, &u.to_string(), t.muted()),
                    _ => 0,
                }
            } else {
                0
            };
            put(f, area, 1, 1, vec![Span::styled(if folded { "▸" } else { "▾" }, t.faint())]);
            let room = area.width.saturating_sub(3 + used + 2);
            let name = clip(s.name.line().as_str(), usize::from(room));
            put(f, area, 3, room, vec![Span::styled(name, t.section())]);
            None
        }
        Row::Conversation(i) => {
            let c = app.model.conversation(i);
            let icons = app.settings.icons;
            let prefix = match (&c.kind, icons) {
                (ConversationKind::Channel { private: false }, _) => "#",
                (ConversationKind::Channel { private: true }, false) => "▪",
                (ConversationKind::Channel { private: true }, true) => ICON_PRIVATE,
                (ConversationKind::Dm { .. } | ConversationKind::GroupDm { .. }, _) => "@",
            };
            let unread = c.unread > 0 && !c.muted;
            let (name_style, mut prefix_style) = if c.muted {
                (t.faint(), t.faint())
            } else if unread {
                (t.bold(), t.bold())
            } else {
                (t.text(), t.muted())
            };
            // A DM shows its peer's presence in place of `@`: the shape tells it, the color helps.
            let presence = match &c.kind {
                ConversationKind::Dm { user } => app.model.user(user).and_then(|u| presence_mark(u.presence, icons)),
                _ => None,
            };
            let prefix = match presence {
                Some((mark, p)) => {
                    if !c.muted {
                        prefix_style = t.presence(p);
                    }
                    mark
                }
                None => prefix,
            };
            // Mentions always show; a DM's unread messages unless it is muted.
            let count = if c.mentions > 0 {
                c.mentions
            } else if unread && c.is_dm() {
                c.unread
            } else {
                0
            };
            let pill = if count > 0 { right(f, area, &count_pill(count), t.badge()) } else { 0 };
            // DMs has no sections: its rows start where a section's name would.
            let x = if app.shell.view == View::Home { 3 } else { 1 };
            put(f, area, x, 1, vec![Span::styled(prefix, prefix_style)]);
            let tag = match (c.external, icons) {
                (false, _) => String::new(),
                (true, false) => format!(" {}", app.i18n.label(Label::ListExternal)),
                (true, true) => format!(" {ICON_EXTERNAL}"),
            };
            let tag_w = width(&tag) as u16;
            // One blank cell at least between the name and what is right of it.
            let gap = if pill > 0 { pill + 2 } else { 1 };
            let room = area.width.saturating_sub(x + 2 + gap + tag_w);
            let name = clip(c.name.line().as_str(), usize::from(room));
            let name_w = width(&name) as u16;
            let mut spans = vec![Span::styled(name, name_style)];
            if !tag.is_empty() {
                spans.push(Span::styled(tag, t.faint()));
            }
            put(f, area, x + 2, name_w + tag_w, spans);
            presence.filter(|_| !c.muted).map(|(_, p)| (area.x + x, p))
        }
    }
}
