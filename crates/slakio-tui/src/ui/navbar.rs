//! Navigation in the list panel: the workspace chip on its title and the view switcher under
//! it, a row per view (or one folded row), a rule under them joined to the border. The view the
//! list shows has the selection bar, as the list's cursor row: the focused one while the
//! keyboard is on the views' cursor there, else the unfocused one; the cursor on another view
//! has the focused bar. The geometry is [`crate::navbar`]'s, the same the mouse reads.
//!
//! A Nerd Font glyph goes in one cell whose symbol is the glyph and a blank, so the cell after
//! it is never written on its own: a terminal that draws the glyph two cells wide does not lose
//! the text after it (it is positioned anew), and one that draws it narrow clears that cell.

use crate::app::App;
use crate::app::Focus;
use crate::app::shell::View;
use crate::navbar::Part;
use crate::theme::Selection;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

/// Draw the chip, the view switcher and the rule of list panel `list`.
pub(super) fn draw(f: &mut Frame, app: &App, list: Rect) {
    let t = &app.theme;
    let focused = matches!(app.focus(), Focus::List | Focus::ViewSwitcher);
    let Some(parts) = app.list_parts() else { return };
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
    let on_views = app.focus() == Focus::ViewSwitcher;
    for row in app.view_rows() {
        let Some(view) = row.pieces.first().and_then(|p| p.item).and_then(|i| View::ALL.get(i).copied()) else {
            continue;
        };
        let shown = view == app.shell.view;
        let red = app.view_unread(app.shell.workspace, view).red();
        let buf = f.buffer_mut();
        for p in &row.pieces {
            let style = match p.part {
                Part::Badge => t.dot(red),
                Part::Fold => t.faint(),
                _ if shown => t.bold(),
                _ => t.text(),
            };
            if p.part == Part::Glyph {
                super::glyph_cell(buf, p.x, row.area.y, &p.text, style);
            } else {
                buf.set_string(p.x, row.area.y, &p.text, style);
            }
        }
        let cursor = on_views && View::ALL.get(app.shell.nav_cursor) == Some(&view);
        let how = match (cursor, shown) {
            (true, _) => Selection::Focused,
            (false, true) => Selection::Unfocused,
            (false, false) => continue,
        };
        t.paint_selection(f.buffer_mut(), row.area, how);
    }
}
