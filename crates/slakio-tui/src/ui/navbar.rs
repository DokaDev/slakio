//! The top bar: the workspace chip and the views with their counts, on the status line's
//! surface (the app's own lines; the tab bar below it belongs to the work area). The view the
//! list shows is raised like the tab shown; with the keyboard on the bar its cursor is the
//! selection bar. The geometry is [`crate::navbar`]'s, the same the mouse reads.
//!
//! A Nerd Font glyph goes in one cell whose symbol is the glyph and a blank, so the cell after
//! it is never written on its own: a terminal that draws the glyph two cells wide does not lose
//! the text after it (it is positioned anew), and one that draws it narrow clears that cell.

use crate::app::App;
use crate::app::Focus;
use crate::app::shell::{NavItem, nav_items};
use crate::navbar::{Bar, Part};
use crate::theme::Selection;
use ratatui::Frame;
use ratatui::layout::Rect;

pub(super) fn draw(f: &mut Frame, app: &App, bar: &Bar) {
    let t = &app.theme;
    let ws = app.model.workspaces().get(app.shell.workspace);
    let buf = f.buffer_mut();
    buf.set_style(bar.area, t.nav_bar());
    let y = bar.area.y;
    let items = nav_items();
    for p in &bar.pieces {
        let view = p.item.and_then(|i| match items.get(i) {
            Some(NavItem::View(v)) => Some(*v),
            _ => None,
        });
        let shown = view.is_some_and(|v| v == app.shell.view);
        let style = match p.part {
            Part::Band => ws.map_or(t.nav_bar(), |w| t.nav_bar().patch(t.workspace(w.color))),
            Part::Name => t.nav_bar().add_modifier(ratatui::style::Modifier::BOLD),
            Part::Caret | Part::Sep => t.nav_quiet(),
            Part::Other => t.nav_quiet().add_modifier(ratatui::style::Modifier::BOLD),
            Part::Mark => t.nav_marker(true),
            Part::Badge => t.nav_marker(view.is_some_and(|v| app.view_unread(app.shell.workspace, v).red())),
            Part::Glyph | Part::Label | Part::Blank if view.is_some() => t.nav_item(shown),
            Part::Glyph | Part::Label | Part::Blank => t.nav_bar(),
        };
        // On the view shown, its count keeps the raised background.
        let red = view.is_some_and(|v| app.view_unread(app.shell.workspace, v).red());
        let style = if p.part == Part::Badge && shown { t.nav_shown_marker(red) } else { style };
        if p.part == Part::Glyph {
            super::glyph_cell(buf, p.x, y, &p.text, style);
        } else {
            buf.set_string(p.x, y, &p.text, style);
        }
    }
    if app.focus() == Focus::Nav
        && let Some((from, to)) = bar.span(app.shell.nav_cursor)
    {
        let row = Rect { x: from, y, width: to - from, height: 1 };
        app.theme.paint_selection(f.buffer_mut(), row, Selection::Focused);
    }
}
