//! Navigation in the list panel: the workspace chip on its title and the view switcher under
//! it, a row per view (or one folded row), a rule under them joined to the border with the
//! handle `▾` that folds them. The view the list shows is marked, never barred: an accent `▎`
//! in its first column, its glyph in the accent, its name bold. Bars are for cursors only: the
//! views' cursor has the focused one while the keyboard is there (the marker stays on it). The
//! geometry is [`crate::navbar`]'s, the same the mouse reads.
//!
//! A Nerd Font glyph goes in one cell whose symbol is the glyph and a blank, so the cell after
//! it is never written on its own: a terminal that draws the glyph two cells wide does not lose
//! the text after it (it is positioned anew), and one that draws it narrow clears that cell.

use crate::app::App;
use crate::app::Focus;
use crate::app::Unread;
use crate::app::shell::View;
use crate::navbar::{self, Part};
use crate::theme::{GUTTER, Selection};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

/// Draw the chip, the view switcher and the rule of list panel `list`.
pub(super) fn draw(f: &mut Frame, app: &App, list: Rect) {
    let t = &app.theme;
    let focused = matches!(app.focus(), Focus::List | Focus::ViewSwitcher);
    let Some(parts) = app.list_parts() else { return };
    if let Some(chip) = app.chip_bar() {
        let ws = app.shell.current(&app.model);
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
        // The handle that folds the views, where the folded row's `▸` is.
        if !app.shell.nav_folded && list.width > navbar::FOLD_X + 1 {
            f.buffer_mut().set_string(list.x + navbar::FOLD_X, rule, "▾", t.faint());
        }
    }
    let on_views = app.focus() == Focus::ViewSwitcher;
    for row in app.view_rows() {
        let Some(view) = row.pieces.first().and_then(|p| p.item).and_then(|i| View::ALL.get(i).copied()) else {
            continue;
        };
        let shown = view == app.shell.view;
        let own = app.view_unread(app.workspace(), view);
        let hidden = app.hidden_mark();
        let buf = f.buffer_mut();
        for p in &row.pieces {
            let style = match p.part {
                // Counts Slack could not tell: unknown, faint.
                Part::Badge if p.text == "?" => t.faint(),
                Part::Badge => count_style(t, own),
                // A hidden view's mark on the folded row, styled as its own row would be.
                Part::Mark => hidden.map_or_else(|| t.bold(), |(_, u)| count_style(t, u)),
                Part::MarkGlyph | Part::Other => t.muted(),
                Part::Blank if p.text.contains('·') => t.muted(),
                Part::Fold => t.faint(),
                Part::Glyph if shown => t.current(),
                _ if shown => t.bold(),
                _ => t.text(),
            };
            if p.part.is_glyph() {
                super::glyph_cell(buf, p.x, row.area.y, &p.text, style);
            } else {
                buf.set_string(p.x, row.area.y, &p.text, style);
            }
        }
        // Bars are the cursor's alone; the view shown has its marker, on the bar too.
        if on_views && View::ALL.get(app.shell.nav_cursor) == Some(&view) {
            t.paint_selection(f.buffer_mut(), row.area, Selection::Focused);
        }
        if shown {
            let current = t.current();
            let cell = &mut f.buffer_mut()[(row.area.x, row.area.y)];
            if let Some(fg) = current.fg {
                cell.set_fg(fg);
            }
            cell.set_symbol(GUTTER).modifier.insert(Modifier::BOLD);
        }
    }
}

/// A view's count: only a mention is red; to-dos are muted, in the warning color when one is
/// overdue.
fn count_style(t: &crate::theme::Theme, u: Unread) -> Style {
    match u {
        Unread { todos: n, overdue, .. } if n > 0 && u.level() == 0 => {
            if overdue {
                t.warning()
            } else {
                t.muted()
            }
        }
        _ => t.dot(u.red()),
    }
}
