//! The list panel: under its workspace chip and views ([`super::navbar`]), the view picked
//! there. Home lists the workspace's sidebar sections
//! (folded with `Enter`) and their conversations, a blank row between two sections; DMs lists
//! the DMs, To-dos its to-dos and Home ends with the saved messages ([`super::todos`]),
//! Activity the user's notices ([`super::activity`]). Views built in a later version say so and
//! how to get back.
//!
//! ```text
//! ╭ ▌A company ▾ ─────────────╮
//! │ 󰋜 Home                 @3 │   the views, a row each ([`super::navbar`])
//! │ …                         │
//! ├───────────────────────────┤
//! │ ▾ Favorites               │   col 0 gutter, col 1 fold mark, col 3 the section's name
//! │   # backend               │   col 3 prefix, col 5 name
//! │   # incidents         3   │   a pill: mentions (or unread DM messages)
//! │                           │
//! │ ▸ Ops                 2   │   folded: how many of its conversations are unread
//! │   ▪ secret-proj           │   ▪ = private
//! │   ⌗ partner-shared        │   ⌗ = shared with another organization
//! │   # feed-customer-1…  1   │   names end in … where they would touch the pill
//! │   MK● Minsu Kim           │   a DM: its peer's avatar chip, and at its corner whether
//! │   JL◐ Jiyoung Lee         │   they are active (●), away (○) or in do not disturb (◐)
//! │   3   Jiho Park, …        │   a group DM: how many people are in it
//! ```
//!
//! With `avatars = "image"` a DM's chip is its peer's photo once the terminal holds it
//! ([`photo`]); the initials until then and wherever pictures are not drawn.
//!
//! With `avatars = "off"` a DM starts with the presence mark alone (`● Minsu Kim`, `@` while
//! unknown) and a group DM with `@`.
//!
//! Unread conversations are bold in body text, read ones muted, muted ones faint and never
//! badged; nothing else marks them.

mod photo;

use super::{avatar_chip, count_pill, frame, highlight, presence_mark};
use crate::app::model::Row;
use crate::app::shell::{Region, View};
use crate::app::{App, Focus};
use crate::avatar;
use crate::group_name;
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

/// Nerd Font glyphs: `nf-md-lock` (a private channel), `nf-md-link_variant` (a DM with another
/// organization) and `nf-md-share_variant` (a channel shared with another organization).
const ICON_PRIVATE: &str = "\u{F033E}";
const ICON_EXTERNAL: &str = "\u{F0339}";
pub(super) const ICON_SHARED: &str = "\u{F0497}";
/// A channel shared with another organization with icons off: one cell wide on every terminal.
pub(super) const SHARED_MARK: &str = "\u{2317}";

pub(super) fn draw(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    // The title is the workspace chip ([`super::navbar`]).
    f.render_widget(frame(app, Region::List, ""), area);
    let inner = screen::list_parts(area, app.nav_rows()).rows;
    if inner.is_empty() {
        return;
    }
    let rows = app.shell.rows(&app.model);
    // Unknown is not empty: what could not be loaded, or is not available, says so.
    let faint = |l: Label| ratatui::text::Text::styled(app.i18n.label(l).to_string(), t.faint());
    let ws = app.shell.current_id(&app.model);
    let unknown = match app.boot_failed() {
        _ if ws.as_ref().is_some_and(|w| app.model.is_loaded_in(w)) => {
            let known = ws.as_ref().is_some_and(|w| app.model.saved_known(w));
            (app.shell.view == View::Todos && !known).then(|| faint(Label::ListUnavailable))
        }
        Some(kind) => {
            let what = if app.boots_alone() { Label::ListFailedOne } else { Label::ListFailed };
            Some(match app.failed_why() {
                // It cannot be asked again: why, and what to do, under it (in body text, apart
                // from the faint "Loading…").
                Some(why) => {
                    let mut text = ratatui::text::Text::styled(app.i18n.label(what).to_string(), t.text());
                    let lines = crate::text::wrap(&app.i18n.label(why), usize::from(pad(inner).width), 4).0;
                    text.lines.extend(lines.into_iter().map(|l| Line::styled(l, t.muted())));
                    text
                }
                None => super::failed_text(app, what, kind.retryable()),
            })
        }
        None => Some(faint(Label::ListLoading)),
    };
    if let Some(text) = unknown {
        let p = Paragraph::new(text);
        f.render_widget(p, pad(inner));
        return;
    }
    if app.shell.view == View::Activity && rows.is_empty() {
        let text = match app.activity_unloaded() {
            Some(None) => faint(Label::ListLoading),
            Some(Some(kind)) => super::failed_text(app, Label::ActivityFailed, kind.retryable()),
            None => faint(Label::ActivityEmpty),
        };
        f.render_widget(Paragraph::new(text).wrap(ratatui::widgets::Wrap { trim: true }), pad(inner));
        return;
    }
    if app.shell.view.is_placeholder() || rows.is_empty() {
        super::empty::list(f, app, inner);
        return;
    }
    let focused = app.focus() == Focus::List;
    // Activity keeps its last row for its end (`App::list_height`).
    let shown = if app.shell.view == View::Activity { app.list_height() } else { usize::from(inner.height) };
    // The row under the mouse (an item with its second line), unless the cursor is on it.
    let hovered = app.hovered_list_row().filter(|r| *r != app.shell.list_cursor);
    for (k, &row) in rows.iter().enumerate().skip(app.shell.list_top).take(shown) {
        let y = inner.y + (k - app.shell.list_top) as u16;
        let area = Rect { y, height: 1, ..inner };
        let mark = draw_row(f, app, row, area);
        // A to-do's second line is under the cursor with its first.
        let item_meta = matches!(row, Row::ItemMeta(_) | Row::NoticeMeta(_));
        let on_item = item_meta && k == app.shell.list_cursor + 1;
        if on_item {
            highlight(f, app, area, if focused { Selection::Focused } else { Selection::Unfocused });
        }
        if hovered.is_some_and(|h| h == k || item_meta && h + 1 == k) {
            super::hover::row(f, app, area);
        }
        if k == app.shell.list_cursor && row.is_selectable() {
            highlight(f, app, area, if focused { Selection::Focused } else { Selection::Unfocused });
            // A presence mark keeps its color on the bar where it reads there.
            if let Some((x, fg)) = mark.and_then(|(x, p)| t.presence_selected(p).map(|fg| (x, fg))) {
                f.buffer_mut()[(x, y)].set_fg(fg);
            }
        }
    }
    // Activity's end: a page on its way, or one that failed, under the last notice (on the row
    // kept for it when the list fills the panel).
    let drawn = rows.len().saturating_sub(app.shell.list_top).min(shown);
    if app.shell.view == View::Activity && drawn < usize::from(inner.height) {
        super::activity::foot(f, app, Rect { y: inner.y + drawn as u16, height: 1, ..inner });
    }
}

/// Inside the gutter and the right padding.
fn pad(r: Rect) -> Rect {
    Rect { x: r.x + 1, width: r.width.saturating_sub(2), ..r }
}

/// The row of `c` starts with an avatar chip: avatars are on and it is a DM or a group DM.
fn avatars_on(app: &App, c: &slakio_core::model::Conversation) -> bool {
    app.settings.avatars && c.is_dm()
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

/// The name a section header shows: a section of the user's own keeps the user's name; a built-in
/// one is named in the UI's language, whatever name it holds (an older cache kept Slack's).
fn section_name(app: &App, s: &slakio_core::model::Section) -> String {
    use slakio_core::model::SectionKind;
    let own = s.name.line();
    if s.kind == SectionKind::Custom && !own.as_str().trim().is_empty() {
        return own.into_string();
    }
    let label = match s.kind {
        SectionKind::Favorites => Label::SectionStarred,
        SectionKind::Custom => Label::SectionCustom,
        SectionKind::External => Label::SectionExternal,
        SectionKind::Channels => Label::SectionChannels,
        SectionKind::DirectMessages => Label::SectionDms,
        SectionKind::Apps => Label::SectionApps,
    };
    app.i18n.label(label).to_string()
}

/// Draw `row` in `area`; where its presence mark is (the column), and whose, when it has one
/// that is not faint.
#[expect(clippy::too_many_lines, reason = "one arm per row kind; to be split")]
fn draw_row(f: &mut Frame, app: &App, row: Row, area: Rect) -> Option<(u16, Presence)> {
    let t = &app.theme;
    match row {
        Row::Spacer => None,
        Row::Filter | Row::Group(_) | Row::Item(_) | Row::ItemMeta(_) => {
            super::todos::draw(f, app, row, area);
            None
        }
        Row::Notice(i) => {
            super::activity::notice(f, app, i, area);
            None
        }
        Row::NoticeMeta(i) => {
            super::activity::meta(f, app, i, area);
            None
        }
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
            let name = clip(&section_name(app, s), usize::from(room));
            put(f, area, 3, room, vec![Span::styled(name, t.section())]);
            None
        }
        Row::Conversation(i) => {
            let c = app.model.conversation(i);
            let icons = app.settings.icons;
            // A private channel keeps its lock when shared too: access is the stronger fact.
            let prefix = match (&c.kind, icons) {
                (ConversationKind::Channel { private: false }, false) if c.external => SHARED_MARK,
                (ConversationKind::Channel { private: false }, true) if c.external => ICON_SHARED,
                (ConversationKind::Channel { private: false }, _) => "#",
                (ConversationKind::Channel { private: true }, false) => "▪",
                (ConversationKind::Channel { private: true }, true) => ICON_PRIVATE,
                (ConversationKind::Dm { .. } | ConversationKind::GroupDm { .. }, _) => "@",
            };
            let unread = c.unread > 0 && !c.muted;
            let (name_style, mut prefix_style) = if c.muted {
                (t.faint(), t.faint())
            } else {
                (t.row_name(unread), if unread { t.bold() } else { t.muted() })
            };
            // A DM shows its peer's presence in place of `@`: the shape tells it, the color helps.
            let presence = match &c.kind {
                ConversationKind::Dm { user } => {
                    app.model.user(&c.workspace, user).and_then(|u| presence_mark(u.presence))
                }
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
            // A DM: its unread messages (unless muted), red only when one mentions the user. A
            // channel: its mentions, red. Red is for mentions only (ui-ux-spec).
            // Muted: faint; a channel shows its mentions only, a DM nothing.
            let (unread_n, mentions) = c.badges();
            let count = match (c.is_dm(), unread) {
                (true, true) => unread_n,
                _ => mentions,
            };
            let style = if mentions > 0 { t.badge() } else { t.pill() };
            let pill = if count > 0 { right(f, area, &count_pill(count), style) } else { 0 };
            // DMs has no sections: its rows start where a section's name would.
            let x = if app.shell.view == View::Home { 3 } else { 1 };
            // With avatars, a DM starts with its peer's chip and the presence mark at its
            // corner, a group DM with a chip of how many people are in it; the name follows. A
            // DM's photo takes the chip's cells; muted, it stays as it is (its colour says which
            // picture it is).
            let photo = match &c.kind {
                ConversationKind::Dm { user } if avatars_on(app, c) => photo::chip(app, &c.workspace, user),
                _ => None,
            };
            let is_photo = photo.is_some();
            let group = match &c.kind {
                ConversationKind::GroupDm { users } => Some(group_name::count(users.len())),
                _ => None,
            };
            let chip = avatars_on(app, c).then(|| match &c.kind {
                ConversationKind::Dm { .. } if is_photo => photo,
                ConversationKind::Dm { user } => avatar_chip(app, &c.workspace, user, c.name.line().as_str()),
                ConversationKind::GroupDm { .. } => {
                    group.as_ref().map(|n| Span::styled(format!("{n:<2}"), t.avatar_group()))
                }
                ConversationKind::Channel { .. } => None,
            });
            let (mark_x, name_x) = match (chip.flatten(), group) {
                (Some(mut chip), _) => {
                    if c.muted && !is_photo {
                        chip.style = t.avatar_muted();
                    }
                    put(f, area, x, avatar::WIDTH as u16, vec![chip]);
                    if let Some((mark, _)) = presence {
                        put(f, area, x + 2, 1, vec![Span::styled(mark, prefix_style)]);
                    }
                    (x + 2, x + 4)
                }
                // Without avatars, a group DM starts with how many people are in it: `8  Ann, …`.
                (None, Some(n)) => {
                    put(f, area, x, 2, vec![Span::styled(n, prefix_style)]);
                    (x, x + 3)
                }
                (None, None) => {
                    if prefix == ICON_PRIVATE || prefix == ICON_SHARED {
                        super::glyph_cell(f.buffer_mut(), area.x + x, area.y, prefix, prefix_style);
                    } else {
                        put(f, area, x, 1, vec![Span::styled(prefix, prefix_style)]);
                    }
                    (x, x + 2)
                }
            };
            // Only a DM ends with its link; a shared channel says so by its prefix.
            let external = c.external && c.is_dm();
            let mut tag = match (external, icons) {
                (false, _) => String::new(),
                (true, false) => format!(" {}", app.i18n.label(Label::ListExternal)),
                // The glyph and the blank of its slot.
                (true, true) => format!(" {ICON_EXTERNAL} "),
            };
            // A bot's row says it is an app, with its photo (whatever picture it chose, a
            // person's even) or its initials.
            let bot = match &c.kind {
                ConversationKind::Dm { user } => app.model.user(&c.workspace, user).is_some_and(|u| u.bot),
                _ => false,
            };
            if bot {
                tag.push_str(&format!(" {}", app.i18n.label(Label::MessageApp)));
            }
            let tag_w = width(&tag) as u16;
            // One blank cell at least between the name and what is right of it.
            let gap = if pill > 0 { pill + 2 } else { 1 };
            let room = area.width.saturating_sub(name_x + gap + tag_w);
            // A group DM named by its people: two of them and how many more, as the room allows.
            let name = match group_name::people(&app.model, c) {
                Some(names) => group_name::fit(&names, &app.i18n, usize::from(room)),
                None => clip(c.name.line().as_str(), usize::from(room)),
            };
            let name_w = width(&name) as u16;
            let mut spans = vec![Span::styled(name, name_style)];
            if !tag.is_empty() {
                spans.push(Span::styled(tag, t.faint()));
            }
            put(f, area, name_x, name_w + tag_w, spans);
            if icons && external {
                super::glyph_cell(f.buffer_mut(), area.x + name_x + name_w + 1, area.y, ICON_EXTERNAL, t.faint());
            }
            presence.filter(|_| !c.muted).map(|(_, p)| (area.x + mark_x, p))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SHARED_MARK;

    /// The shared mark is one cell wide on every terminal, also where East Asian ambiguous width
    /// is drawn wide (common with Korean), so a row's name never shifts.
    #[test]
    fn the_shared_mark_is_one_cell_wide_on_ambiguous_wide_terminals_too() {
        use unicode_width::UnicodeWidthStr;
        assert_eq!((SHARED_MARK.width(), SHARED_MARK.width_cjk()), (1, 1));
    }
}
