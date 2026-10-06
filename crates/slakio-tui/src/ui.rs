//! Drawing a frame of [`App`]. Without a backend: the welcome text and the status line. With
//! one: the shell — rail, list panel, work area — over the status line (see [`crate::screen`]
//! for the geometry), or, on a terminal too small for it, only how much room it needs. Popups
//! go on top: the which-key popup above the status line, the keyboard help and a question over
//! the dimmed screen.
//!
//! * [`rail`], [`list`], [`work`] — the three regions; [`timeline`] — a pane's messages;
//!   [`statusline`] — the bottom line with its hints; [`empty`] — empty states that list keys;
//!   [`guide`], [`help`], [`dialog`] — the popups.
//!
//! Names and messages drawn here are remote text ([`slakio_core::sanitize::Remote`]): it can
//! only be drawn through the sanitiser (`line()`, `block()`), never as it came. A test keeps
//! `Remote::unsanitized` out of this module.

mod dialog;
mod empty;
mod guide;
mod help;
mod list;
mod rail;
mod statusline;
pub mod timeline;
mod work;

use crate::app::App;
use crate::app::shell::{Region, View};
use crate::screen;
use crate::text::clip;
use crate::theme::Selection;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use slakio_core::i18n::{Label, Localized, Msg};
use slakio_core::model::Presence;
use slakio_core::sanitize::Safe;
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
        let a = app.areas();
        if let Some(l) = a.list {
            list::draw(f, app, l);
        }
        work::draw(f, app, a.work);
        // An expanded rail over the list panel clears only its own cells: the list shows
        // beside it.
        rail::draw(f, app, a.rail);
    }
    statusline::draw(f, app, status, now);
    if app.which_key_visible(now) && app.help.is_none() && app.dialog.is_none() {
        app.theme.dim_area(f.buffer_mut(), body);
        guide::draw(f, app, body);
    }
    if app.help.is_some() {
        app.theme.dim_area(f.buffer_mut(), body);
        help::draw(f, app, body);
    }
    if app.dialog.is_some() {
        app.theme.dim_area(f.buffer_mut(), area);
        dialog::draw(f, app, body);
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
    panel(app, app.shell.focus == region, Line::from(title_span(app, title, app.shell.focus == region)))
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

/// The name of a view (rail labels, list title, breadcrumb).
fn view_label(view: View) -> Label {
    match view {
        View::Home => Label::RailHome,
        View::Dms => Label::RailDms,
        View::Activity => Label::RailActivity,
        View::Files => Label::RailFiles,
        View::Later => Label::RailLater,
    }
}

/// The rail's letter, or its Nerd Font icon, for a view.
fn view_glyph(view: View, icons: bool) -> &'static str {
    match (view, icons) {
        (View::Home, false) => "H",
        (View::Dms, false) => "D",
        (View::Activity, false) => "A",
        (View::Files, false) => "F",
        (View::Later, false) => "L",
        (View::Home, true) => "\u{F02DC}",
        (View::Dms, true) => "\u{F0361}",
        (View::Activity, true) => "\u{F009A}",
        (View::Files, true) => "\u{F0219}",
        (View::Later, true) => "\u{F00C0}",
    }
}

/// A person's presence mark: `●` active, `○` away, `◐` (with icons, a moon) in do not
/// disturb; none while unknown. Told apart by shape, so it reads without color.
fn presence_mark(p: Presence, icons: bool) -> Option<(&'static str, Presence)> {
    let mark = match (p, icons) {
        (Presence::Active, _) => "●",
        (Presence::Away, _) => "○",
        (Presence::Dnd, false) => "◐",
        // nf-md-weather_night
        (Presence::Dnd, true) => "\u{F0594}",
        (Presence::Unknown, _) => return None,
    };
    Some((mark, p))
}

/// The first letter of a workspace's name, upper case: its rail letter.
fn workspace_letter(name: &Safe) -> String {
    name.as_str().chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default()
}

/// The pill of count `n` (` 3 `, ` 99+ `).
fn count_pill(n: u32) -> String {
    if n > 99 { " 99+ ".to_string() } else { format!(" {n} ") }
}

/// Paint the selection over a whole row; its first cell is the gutter.
fn highlight(f: &mut Frame, app: &App, row: Rect, how: Selection) {
    app.theme.paint_selection(f.buffer_mut(), row, how);
}
