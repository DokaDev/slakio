//! Drawing a frame of [`App`]. Without a backend: the welcome text and the status line. With
//! one: the shell — the list panel (navigation: the workspace, the views, the list) and the work
//! area — over the status line (see [`crate::screen`] for the geometry), or, on a terminal too small for it, only how much room it needs. Popups
//! go on top: the which-key popup above the status line, the keyboard help, the command palette
//! and a question over the dimmed screen.
//!
//! * [`list`], [`work`] — the two regions; [`navbar`] — the list panel's workspace chip and
//!   view switcher (and [`switcher`], the workspace switcher); [`tabbar`] — the work area's
//!   tabs, when there are two or more; [`timeline`] — a pane's messages;
//!   [`statusline`] — the bottom line with its hints; [`empty`] — empty states that list keys;
//!   [`guide`], [`help`], [`palette`], [`dialog`] — the popups.
//!
//! Names and messages drawn here are remote text ([`slakio_core::sanitize::Remote`]): it can
//! only be drawn through the sanitiser (`line()`, `block()`), never as it came. A test keeps
//! `Remote::unsanitized` out of this module.

mod dialog;
mod empty;
mod guide;
mod help;
mod list;
mod navbar;
mod palette;
mod statusline;
mod switcher;
mod tabbar;
pub mod timeline;
mod work;

use crate::app::Layer;
use crate::app::shell::{Region, View};
use crate::app::{App, Focus};
use crate::avatar;
use crate::screen;
use crate::text::clip;
use crate::theme::Selection;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use slakio_core::i18n::{Label, Localized, Msg};
use slakio_core::model::{Presence, UserId};
use std::time::Instant;

/// Draw the whole screen at `now` (notices that ran out are not drawn).
pub fn draw(f: &mut Frame, app: &App, now: Instant) {
    let area = f.area();
    f.buffer_mut().set_style(area, app.theme.base());
    let [body, status] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
    if app.backend.is_none() {
        draw_welcome(f, app, body);
    } else if screen::too_small(area) {
        draw_too_small(f, app, area);
        return;
    } else {
        let (layout, views) = app.frame_with_views();
        let a = layout.areas;
        if let Some(l) = a.list {
            list::draw(f, app, l);
            navbar::draw(f, app, l);
        }
        work::draw(f, app, &layout, &views);
        if let Some(bar) = app.tab_bar() {
            tabbar::draw(f, app, &bar);
        }
    }
    statusline::draw(f, app, status, now);
    // The popups bottom up, in the order the keys and the mouse follow; the palette and the
    // which-key popup only on top.
    let layers = app.layers(Some(now));
    for (i, layer) in layers.iter().enumerate().rev() {
        let top = i == 0;
        match layer {
            Layer::WhichKey if top => {
                app.theme.dim_area(f.buffer_mut(), body);
                guide::draw(f, app, body);
            }
            Layer::Palette if top && !screen::too_small(area) => {
                // The whole screen dims, the mode badge stays; the palette goes on top.
                app.theme.dim_area(f.buffer_mut(), area);
                statusline::badge(f, app, status);
                palette::draw(f, app);
            }
            Layer::Help => {
                app.theme.dim_area(f.buffer_mut(), body);
                help::draw(f, app, body);
            }
            Layer::Dialog => {
                app.theme.dim_area(f.buffer_mut(), area);
                dialog::draw(f, app, body);
            }
            Layer::Switcher if !screen::too_small(area) => switcher::draw(f, app),
            Layer::WhichKey | Layer::Palette | Layer::Switcher => {}
        }
    }
}

fn draw_welcome(f: &mut Frame, app: &App, area: Rect) {
    use crate::action::{Action, AppAction, HelpAction};
    use crate::keymap::Ctx;
    let t = &app.theme;
    let content = empty::Content {
        headline: Some((Localized::verbatim("slakio").to_string(), t.current())),
        sentence: Some(app.i18n.label(Label::WelcomeStatus).to_string()),
        keys: empty::Content::keys(
            app,
            Ctx::Root,
            &[(Action::App(AppAction::Quit), Label::EmptyQuit), (Action::Help(HelpAction::Open), Label::EmptyHelp)],
        ),
        tip: Some(app.i18n.label(Label::EmptyDemo).to_string()),
    };
    content.draw_centered(f, app, area);
}

/// A popup box: cleared, on the surface color, with a title and, at the bottom right, a footer
/// (its keys). Hands back the inside.
fn modal(f: &mut Frame, app: &App, rect: Rect, title: &str, footer: &str) -> Rect {
    let t = &app.theme;
    f.render_widget(Clear, rect);
    f.buffer_mut().set_style(rect, t.surface());
    let w = usize::from(rect.width);
    let title = clip(&format!(" {title} "), w.saturating_sub(4));
    // Cut once, to what fits between the corners and a dash on each side.
    let footer = if footer.is_empty() { String::new() } else { clip(&format!(" {footer} "), w.saturating_sub(4)) };
    let mut block = Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(t.border(true))
        .title(Line::from(Span::styled(title, t.bold())));
    if !footer.is_empty() {
        block = block.title_bottom(Line::from(Span::styled(footer, t.muted())).right_aligned());
    }
    f.render_widget(block, rect);
    screen::inner(rect)
}

fn draw_too_small(f: &mut Frame, app: &App, area: Rect) {
    let msg = Msg::ScreenTooSmall {
        width: area.width.to_string(),
        height: area.height.to_string(),
        min_width: screen::MIN_WIDTH.to_string(),
        min_height: screen::MIN_HEIGHT.to_string(),
    };
    let text = app.i18n.msg(&msg).to_string();
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }).style(app.theme.muted()), area);
}

/// A region's frame: rounded, its border marking the focus.
fn frame<'a>(app: &App, region: Region, title: &str) -> Block<'a> {
    let focused = matches!(
        (region, app.focus()),
        (Region::List, Focus::List | Focus::ViewSwitcher) | (Region::Work, Focus::Pane(_) | Focus::Work)
    );
    panel(app, focused, Line::from(title_span(app, title, focused)))
}

/// A panel: rounded, the focus shown by its border (accent) and its title (bold) and nothing
/// else. The title has a style of its own, never the border's.
fn panel<'a>(app: &App, focused: bool, title: Line<'a>) -> Block<'a> {
    Block::bordered().border_set(border::ROUNDED).border_style(app.theme.border(focused)).title(title)
}

/// A title ` text ` in the style of a focused (or not) panel; empty text gives no title.
fn title_span(app: &App, text: &str, focused: bool) -> Span<'static> {
    if text.is_empty() {
        return Span::raw("");
    }
    Span::styled(format!(" {text} "), app.theme.title(focused))
}

/// The name of a view (the view switcher, the breadcrumb).
pub(crate) fn view_label(view: View) -> Label {
    view.label()
}

/// A person's presence mark: `●` active, `○` away, `◐` in do not disturb; none while
/// unknown. One set of one weight, icons or not; told apart by shape, so it reads without
/// color.
fn presence_mark(p: Presence) -> Option<(&'static str, Presence)> {
    let mark = match p {
        Presence::Active => "●",
        Presence::Away => "○",
        Presence::Dnd => "◐",
        Presence::Unknown => return None,
    };
    Some((mark, p))
}

/// A person's avatar chip: their initials on their color ([`crate::avatar`]); `None` with
/// avatars off. `name` is the sanitised name drawn beside it.
fn avatar_chip(app: &App, id: &UserId, name: &str) -> Option<Span<'static>> {
    if !app.settings.avatars {
        return None;
    }
    let handle = || app.model.user(id).map(|u| u.name.line().as_str().to_string()).unwrap_or_default();
    Some(Span::styled(avatar::initials(name, handle), app.theme.avatar(avatar::slot(id))))
}

/// Nerd Font glyph `g` at `(x, y)` in its two-cell slot: one cell whose symbol is the glyph and a
/// blank, the cell after it blank and never written on its own. A terminal that draws the glyph
/// two cells wide covers just its slot (the text after it is placed by a cursor move); one that
/// draws it narrow clears the slot's second cell.
pub(crate) fn glyph_cell(buf: &mut ratatui::buffer::Buffer, x: u16, y: u16, g: &str, style: ratatui::style::Style) {
    let area = buf.area;
    if x + 1 >= area.right() || y >= area.bottom() {
        return;
    }
    buf[(x, y)].set_symbol(&format!("{g} ")).set_style(style);
    buf[(x + 1, y)].set_symbol(" ").set_style(style);
}

/// The pill of count `n` (` 3 `, ` 99+ `).
fn count_pill(n: u32) -> String {
    if n > 99 { " 99+ ".to_string() } else { format!(" {n} ") }
}

/// Paint the selection over a whole row; its first cell is the gutter.
fn highlight(f: &mut Frame, app: &App, row: Rect, how: Selection) {
    app.theme.paint_selection(f.buffer_mut(), row, how);
}
