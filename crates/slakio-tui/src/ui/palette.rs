//! The command palette over the dimmed screen, in the style of noice.nvim's command line popup:
//! a box near the top, the `:` input on its first line, a rule, then the entries, each what it
//! completes to (accent), what it does, and its keys on the right (muted); the selected entry
//! on the selection bar. Why the last `Enter` ran nothing goes under the entries. The rows come
//! from [`App::palette_rows`]; the geometry from [`crate::screen::palette`], which the mouse
//! uses too.

use crate::app::App;
use crate::text::{clip, width};
use crate::theme::Selection;
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use slakio_core::i18n::Label;

/// The widest "completes to" column; longer names end in `…`.
const NAME_MAX: usize = 32;

pub(super) fn draw(f: &mut Frame, app: &App) {
    let t = &app.theme;
    let Some(b) = app.palette_box_in(f.area()) else { return };
    let rows = app.palette_rows();
    f.render_widget(Clear, b.rect);
    f.buffer_mut().set_style(b.rect, t.surface());
    let w = usize::from(b.rect.width);
    let title = clip(&format!(" {} ", app.i18n.label(Label::PaletteTitle)), w.saturating_sub(4));
    let keys = format!(" {} ", app.i18n.label(Label::PaletteKeys));
    let mut block = Block::bordered()
        .border_set(border::ROUNDED)
        .border_style(t.border(true))
        .title(Line::from(Span::styled(title, t.bold())));
    if width(&keys) + 6 <= w {
        block = block.title_bottom(Line::from(Span::styled(keys, t.muted())).right_aligned());
    }
    f.render_widget(block, b.rect);

    // The input: `:` and what was typed, or what can be typed.
    let input = b.input;
    let iw = usize::from(input.width);
    f.render_widget(Paragraph::new(":").style(t.key()), Rect { x: input.x + 1, width: 1, ..input });
    let text_area = Rect { x: input.x + 3, width: input.width.saturating_sub(4), ..input };
    let typed = app.cmdline.text();
    // The end of a long line stays in view.
    let shown = tail(typed, usize::from(text_area.width.saturating_sub(1)));
    if typed.is_empty() {
        let hint = clip(&app.i18n.label(Label::PalettePlaceholder), usize::from(text_area.width));
        f.render_widget(Paragraph::new(hint).style(t.faint()), text_area);
    } else {
        f.render_widget(Paragraph::new(shown.clone()).style(t.text()), text_area);
    }
    let cx = text_area.x + width(&shown) as u16;
    f.set_cursor_position(Position { x: cx.min(text_area.right().saturating_sub(1)), y: input.y });
    // A rule under the input, joined to the box's sides.
    let rule = format!("├{}┤", "─".repeat(iw));
    f.render_widget(
        Paragraph::new(rule).style(t.border(true)),
        Rect { x: b.rect.x, y: input.y + 1, width: b.rect.width, height: 1 },
    );

    // The entries.
    if rows.is_empty() {
        let empty = clip(&app.i18n.label(Label::PaletteEmpty), iw.saturating_sub(4));
        f.render_widget(Paragraph::new(empty).style(t.faint()), Rect { x: b.list.x + 2, height: 1, ..b.list });
    }
    let name_w = rows.iter().map(|r| width(&r.name)).max().unwrap_or(0).min(NAME_MAX).min(iw / 2);
    let selected = app.cmdline.selected;
    for (k, (i, r)) in rows.iter().enumerate().skip(b.first).take(usize::from(b.list.height)).enumerate() {
        let row = Rect { y: b.list.y + k as u16, height: 1, ..b.list };
        let kw = width(&r.keys);
        let keys_room = if kw > 0 && kw + name_w + 12 <= iw { kw + 2 } else { 0 };
        let mut x = row.x + 2;
        if name_w > 0 {
            let name = clip(&r.name, name_w);
            f.render_widget(
                Paragraph::new(name).style(t.key().add_modifier(Modifier::BOLD)),
                Rect { x, width: name_w as u16, ..row },
            );
            x += name_w as u16 + 2;
        }
        let label_w = usize::from(row.right().saturating_sub(x)).saturating_sub(keys_room + 1);
        f.render_widget(
            Paragraph::new(clip(&r.label, label_w)).style(t.text()),
            Rect { x, width: label_w as u16, ..row },
        );
        if keys_room > 0 {
            let kx = row.right() - 1 - kw as u16;
            f.render_widget(Paragraph::new(r.keys.clone()).style(t.muted()), Rect { x: kx, width: kw as u16, ..row });
        }
        if i == selected {
            app.theme.paint_selection(f.buffer_mut(), row, Selection::Focused);
        }
    }

    // Why the last `Enter` ran nothing.
    for (k, line) in app.palette_error(f.area()).iter().enumerate() {
        let row = Rect { x: b.extra.x + 2, y: b.extra.y + k as u16, width: b.extra.width.saturating_sub(4), height: 1 };
        f.render_widget(Paragraph::new(line.clone()).style(t.warning()), row);
    }
}

/// The end of `s` that fits in `w` cells.
fn tail(s: &str, w: usize) -> String {
    if width(s) <= w {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars().rev() {
        let mut next = c.to_string();
        next.push_str(&out);
        if width(&next) > w {
            break;
        }
        out = next;
    }
    out
}
