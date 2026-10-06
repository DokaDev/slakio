//! Navigation in the list panel: the workspace chip on its title and the view switcher on its
//! first row, a rule under it joined to the border. The view the list shows is raised like the
//! tab shown and spells its name; with the keyboard on the switcher its cursor is the selection
//! bar. The geometry is [`crate::navbar`]'s, the same the mouse reads.
//!
//! A Nerd Font glyph goes in one cell whose symbol is the glyph and a blank, so the cell after
//! it is never written on its own: a terminal that draws the glyph two cells wide does not lose
//! the text after it (it is positioned anew), and one that draws it narrow clears that cell.

use crate::app::App;
use crate::app::Focus;
use crate::app::shell::View;
use crate::navbar::Part;
use crate::screen;
use crate::theme::Selection;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

/// Draw the chip, the view switcher and the rule of list panel `list`.
pub(super) fn draw(f: &mut Frame, app: &App, list: Rect) {
    let t = &app.theme;
    let focused = matches!(app.focus(), Focus::List | Focus::ViewSwitcher);
    let parts = screen::list_parts(list);
    if let Some(chip) = app.chip_bar() {
        let ws = app.model.workspaces().get(app.shell.workspace);
        let title = t.title(focused);
        for p in &chip.pieces {
            let style = match p.part {
                Part::Band => ws.map_or(title, |w| t.workspace(w.color)),
                Part::Caret => t.muted(),
                Part::Other => t.muted().add_modifier(Modifier::BOLD),
                // Only a mention is red (ui-ux-spec §H.1).
                Part::Mark => t.dot(p.text.trim_start().starts_with('@')),
                _ => title,
            };
            f.buffer_mut().set_string(p.x, chip.area.y, &p.text, style);
        }
    }
    if let Some(rule) = parts.rule {
        let line = format!("├{}┤", "─".repeat(usize::from(list.width.saturating_sub(2))));
        f.buffer_mut().set_string(list.x, rule, line, t.border(focused));
    }
    let Some(bar) = app.view_switcher() else { return };
    let y = bar.area.y;
    let buf = f.buffer_mut();
    for p in &bar.pieces {
        let view = p.item.and_then(|i| View::ALL.get(i).copied());
        let shown = view == Some(app.shell.view);
        let red = view.is_some_and(|v| app.view_unread(app.shell.workspace, v).red());
        let style = match p.part {
            Part::Badge => t.tab_badge(shown, red),
            _ if view.is_some() => t.tab(shown),
            _ => t.base(),
        };
        if p.part == Part::Glyph {
            super::glyph_cell(buf, p.x, y, &p.text, style);
        } else {
            buf.set_string(p.x, y, &p.text, style);
        }
    }
    if app.focus() == Focus::ViewSwitcher
        && let Some((from, to)) = bar.span(app.shell.nav_cursor)
    {
        let row = Rect { x: from, y, width: to - from, height: 1 };
        t.paint_selection(f.buffer_mut(), row, Selection::Focused);
    }
}
