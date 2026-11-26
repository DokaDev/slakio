//! The row of the list under the mouse is drawn apart from the cursor's, quieter than it: a
//! channel, a DM, a group DM, a section header, a to-do with its second line. A click on it does
//! what a click does (opens it, folds the section) and moves the cursor there; the wheel scrolls
//! the list, the cursor stays and the hover follows what is under the pointer.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::Color;
use slakio_core::model::ConversationKind;
use slakio_tui::app::model::Row;
use slakio_tui::app::shell::View;
use slakio_tui::theme::{Background, HOVER_GUTTER, Theme, resolve};

const SIZES: [(u16, u16); 2] = [(80, 24), (120, 40)];

fn themes() -> [Theme; 5] {
    [
        resolve("dark", true, Background::Dark),
        resolve("tokyo-night-night", true, Background::Dark),
        resolve("light", true, Background::Light),
        Theme::terminal(),
        Theme::no_color(),
    ]
}

/// Which row of the list a check takes.
type Pick = dyn Fn(&Demo, &Row) -> bool;

fn gutter(t: &Theme) -> bool {
    t.hover == Color::Reset
}

/// The list's rows as laid out.
fn rows_area(d: &Demo) -> Rect {
    d.app.list_parts().expect("the list panel").rows
}

/// The screen row of list row `k`, when it is shown.
fn y_of(d: &Demo, k: usize) -> Option<u16> {
    let a = rows_area(d);
    let top = d.app.list_top();
    (k >= top && k - top < usize::from(a.height)).then(|| a.y + (k - top) as u16)
}

/// The list's rows on screen with a hovered cell.
fn hovered_rows(t: &Theme, buf: &Buffer, a: Rect) -> Vec<u16> {
    (a.top()..a.bottom())
        .filter(|&y| {
            (a.left()..a.right()).any(|x| {
                let c = &buf[(x, y)];
                if gutter(t) { c.symbol() == HOVER_GUTTER } else { c.bg == t.hover }
            })
        })
        .collect()
}

/// The first row shown, not the cursor's, that `pick` takes.
fn find(d: &Demo, pick: impl Fn(&Demo, &Row) -> bool) -> Option<usize> {
    let rows = d.app.list_rows();
    (0..rows.len()).find(|&k| k != d.app.list_cursor() && y_of(d, k).is_some() && pick(d, &rows[k]))
}

fn conversation_of(d: &Demo, row: &Row, kind: fn(&ConversationKind) -> bool) -> bool {
    matches!(row, Row::Conversation(i) if kind(&d.app.model.conversation(*i).kind))
}

/// The list's cursor row is drawn as the selection, the row `k` under the pointer quietly, and
/// nothing else is hovered.
fn check(d: &mut Demo, t: &Theme, k: usize, lines: usize, what: &str) {
    let a = rows_area(d);
    let y = y_of(d, k).unwrap();
    d.mouse(MouseEventKind::Moved, a.x + 3, y);
    let buf = d.buffer();
    let hovered = hovered_rows(t, &buf, a);
    let want: Vec<u16> = (y..y + lines as u16).filter(|y| *y < a.bottom()).collect();
    assert_eq!(hovered, want, "{what}: the row under the pointer, and only it");
    if !gutter(t) {
        for &y in &want {
            for x in a.left()..a.right() {
                let c = &buf[(x, y)];
                assert!(c.bg != t.bg && c.bg != t.selection, "{what}: ({x}, {y}) {c:?}");
            }
        }
        // (A row brought into view may have scrolled the cursor's out of it.)
        if let Some(cursor) = y_of(d, d.app.list_cursor()) {
            assert!(!hovered.contains(&cursor), "{what}: the cursor's row keeps its look");
            assert!((a.left()..a.right()).any(|x| buf[(x, cursor)].bg == t.selection), "{what}: the cursor row");
        }
    }
}

#[test]
fn a_hovered_list_row_is_quiet_and_the_cursor_row_strong_at_80_and_120() {
    for (w, h) in SIZES {
        for t in themes() {
            let what = format!("{} {w}x{h}", t.name);
            let mut d = Demo::new(w, h);
            d.app.theme = t.clone();
            d.list_cursor_on("general");
            assert!(hovered_rows(&t, &d.buffer(), rows_area(&d)).is_empty(), "{what}: nothing before a move");
            // (The header of the section at the top is drawn on the divider above the rows.)
            let picks: [(&str, Box<Pick>); 4] = [
                ("a section header", Box::new(|_, r| matches!(r, Row::Section(_)))),
                (
                    "a channel",
                    Box::new(|d, r| conversation_of(d, r, |k| matches!(k, ConversationKind::Channel { .. }))),
                ),
                ("a DM", Box::new(|d, r| conversation_of(d, r, |k| matches!(k, ConversationKind::Dm { .. })))),
                (
                    "a group DM",
                    Box::new(|d, r| conversation_of(d, r, |k| matches!(k, ConversationKind::GroupDm { .. }))),
                ),
            ];
            for (name, pick) in picks {
                // Below the panel: the cursor brings it into view, then steps past it.
                let k = match find(&d, &pick) {
                    Some(k) => k,
                    None => {
                        let rows = d.app.list_rows();
                        let k = (0..rows.len()).rfind(|&k| pick(&d, &rows[k])).unwrap_or_else(|| panic!("no {name}"));
                        d.app.focus_list_row(k);
                        d.screen();
                        d.keys(if k + 1 < rows.len() { "j" } else { "k" });
                        d.screen();
                        k
                    }
                };
                assert!(y_of(&d, k).is_some() && d.app.list_cursor() != k, "{what}: {name} shown");
                check(&mut d, &t, k, 1, &format!("{what} {name}"));
            }
            // A to-do: its two lines.
            let mut d = Demo::new(w, h);
            d.app.theme = t.clone();
            d.app.show_unfolded(View::Todos);
            d.screen();
            let rows = d.app.list_rows();
            let items: Vec<usize> = (0..rows.len()).filter(|&k| matches!(rows[k], Row::Item(_))).collect();
            assert!(items.len() > 1, "{what}: to-dos to hover");
            d.app.focus_list_row(items[0]);
            d.screen();
            let k = items[1];
            assert!(y_of(&d, k + 1).is_some(), "{what}: a to-do shown whole");
            check(&mut d, &t, k, 2, &format!("{what} a to-do"));
            // Its second line hovers it too.
            let a = rows_area(&d);
            assert!(!d.mouse(MouseEventKind::Moved, a.x + 3, y_of(&d, k + 1).unwrap()), "{what}: the same to-do");
        }
    }
}

#[test]
fn clicking_a_hovered_conversation_opens_it_and_moves_the_cursor() {
    let t = resolve("dark", true, Background::Dark);
    for (w, h) in SIZES {
        let mut d = Demo::new(w, h);
        d.app.theme = t.clone();
        d.list_cursor_on("general");
        let k = find(&d, |d, r| matches!(r, Row::Conversation(i) if d.app.model.conversation(*i).name == "backend"))
            .expect("backend shown");
        let (a, y) = (rows_area(&d), y_of(&d, k).unwrap());
        d.mouse(MouseEventKind::Moved, a.x + 3, y);
        assert_eq!(hovered_rows(&t, &d.buffer(), a), vec![y]);
        assert!(d.mouse(MouseEventKind::Down(MouseButton::Left), a.x + 3, y));
        d.mouse(MouseEventKind::Up(MouseButton::Left), a.x + 3, y);
        d.pump();
        assert_eq!(d.app.list_cursor(), k, "{w}x{h}: the cursor is on it");
        let p = d.pane(slakio_tui::app::PaneKind::Conversation).expect("a conversation open");
        assert!(matches!(p.target(), slakio_core::model::Target::Conversation { .. }));
        assert!(hovered_rows(&t, &d.buffer(), a).is_empty(), "{w}x{h}: the cursor's row is not hovered");
    }
}

#[test]
fn a_hovered_section_header_folds_on_click() {
    let t = resolve("dark", true, Background::Dark);
    for (w, h) in SIZES {
        let mut d = Demo::new(w, h);
        d.app.theme = t.clone();
        d.list_cursor_on("general");
        let k = find(&d, |_, r| matches!(r, Row::Section(_))).expect("a header shown");
        let (a, y) = (rows_area(&d), y_of(&d, k).unwrap());
        let before = d.app.list_rows().len();
        d.mouse(MouseEventKind::Moved, a.x + 3, y);
        assert_eq!(hovered_rows(&t, &d.buffer(), a), vec![y]);
        d.mouse(MouseEventKind::Down(MouseButton::Left), a.x + 3, y);
        d.mouse(MouseEventKind::Up(MouseButton::Left), a.x + 3, y);
        assert!(d.app.list_rows().len() < before, "{w}x{h}: the section folded");
        assert_eq!(d.app.list_cursor(), k, "{w}x{h}: the cursor on its header");
        assert!(hovered_rows(&t, &d.buffer(), a).is_empty(), "{w}x{h}: the cursor's row is not hovered");
    }
}

#[test]
fn the_wheel_over_the_list_scrolls_without_moving_the_cursor_and_the_hover_follows() {
    let t = resolve("dark", true, Background::Dark);
    for (w, h) in SIZES {
        let mut d = Demo::new(w, h);
        d.app.theme = t.clone();
        d.list_cursor_on("general");
        d.keys("g g");
        d.screen();
        let cursor = d.app.list_cursor();
        let a = rows_area(&d);
        let rows = d.app.list_rows();
        // A row that stays a selectable row once the list moved.
        let mut done = false;
        for y in a.y + 1..a.bottom() {
            let mut e = Demo::new(w, h);
            e.app.theme = t.clone();
            e.list_cursor_on("general");
            e.keys("g g");
            e.screen();
            e.mouse(MouseEventKind::Moved, a.x + 3, y);
            assert!(e.mouse(MouseEventKind::ScrollDown, a.x + 3, y));
            let top = e.app.list_top();
            assert!(top > 0, "{w}x{h}: the list scrolled");
            let k = top + usize::from(y - a.y);
            if !rows.get(k).is_some_and(|r| r.is_selectable()) || k == e.app.list_cursor() {
                continue;
            }
            assert_eq!(e.app.list_cursor(), cursor, "{w}x{h}: the cursor stays");
            assert_eq!(hovered_rows(&t, &e.buffer(), a), vec![y], "{w}x{h}: the row now under the pointer");
            done = true;
            break;
        }
        assert!(done, "{w}x{h}: a row to hover after a notch");
    }
}
