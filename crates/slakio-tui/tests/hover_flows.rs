//! The message under the mouse is drawn apart from the selection, quieter than it: a wash of
//! the theme's hover colour over its rows (a thin bar in the gutter where the theme has none).
//! Only a motion says where the pointer is (a click or the wheel says nothing of it); a move
//! within one message draws nothing; a click turns the hover into the selection; the wheel moves
//! the view and the hover follows what is under the pointer; leaving the pane or the terminal
//! losing the focus takes the hover away.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::Color;
use slakio_tui::app::PaneKind;
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

/// The theme marks the hovered row in its gutter, having no hover colour.
fn gutter(t: &Theme) -> bool {
    t.hover == Color::Reset
}

/// The demo in `theme` at `size`, `general` open with its newest message selected.
fn open(theme: &Theme, (w, h): (u16, u16)) -> Demo {
    let mut d = Demo::new(w, h);
    d.app.theme = theme.clone();
    d.open("general");
    let n = d.pane(PaneKind::Conversation).unwrap().messages().len();
    d.app.select_message(n - 1);
    d.screen();
    d
}

fn area(d: &Demo) -> Rect {
    d.app.message_area(d.pane(PaneKind::Conversation).unwrap().handle()).unwrap()
}

/// The screen rows of message `i` as last drawn.
fn rows_of(d: &Demo, i: usize) -> Vec<u16> {
    d.pane(PaneKind::Conversation).unwrap().drawn_rows().into_iter().filter(|(m, _)| *m == i).map(|(_, y)| y).collect()
}

/// The newest message drawn that is not the selected one, and one of its rows.
fn other(d: &Demo) -> (usize, u16) {
    let p = d.pane(PaneKind::Conversation).unwrap();
    let sel = p.selected();
    let i = p.drawn_rows().into_iter().map(|(m, _)| m).filter(|m| Some(*m) != sel).max().expect("another message");
    let rows = rows_of(d, i);
    (i, rows[rows.len() / 2])
}

/// The rows of the screen with a hovered cell on them: the hover colour, or the gutter's bar.
fn hovered_rows(t: &Theme, buf: &Buffer) -> Vec<u16> {
    let a = buf.area;
    (a.top()..a.bottom())
        .filter(|&y| {
            (a.left()..a.right()).any(|x| {
                let c = &buf[(x, y)];
                if gutter(t) { c.symbol() == HOVER_GUTTER } else { c.bg == t.hover }
            })
        })
        .collect()
}

fn moved(d: &mut Demo, x: u16, y: u16) -> bool {
    d.mouse(MouseEventKind::Moved, x, y)
}

#[test]
fn hover_and_selection_paint_exact_cells_at_80_and_120() {
    for size in SIZES {
        for t in themes() {
            let what = format!("{} {size:?}", t.name);
            let mut d = open(&t, size);
            assert!(hovered_rows(&t, &d.buffer()).is_empty(), "{what}: nothing hovered before a move");
            let a = area(&d);
            let sel = d.pane(PaneKind::Conversation).unwrap().selected().unwrap();
            let (i, y) = other(&d);
            assert!(moved(&mut d, a.x + 5, y), "{what}: a move onto a message draws");
            let buf = d.buffer();
            let hovered = hovered_rows(&t, &buf);
            let mine = rows_of(&d, i);
            assert!(!hovered.is_empty(), "{what}: the message under the pointer is drawn apart");
            assert!(hovered.iter().all(|y| mine.contains(y)), "{what}: only its rows: {hovered:?} of {mine:?}");
            for &y in &hovered {
                if gutter(&t) {
                    assert_eq!(buf[(a.x, y)].symbol(), HOVER_GUTTER, "{what}: the bar is in the gutter");
                } else {
                    // Every cell of a hovered row is washed, but what keeps its own colour.
                    for x in a.left()..a.right() {
                        let c = &buf[(x, y)];
                        assert!(c.bg != t.bg && c.bg != t.selection, "{what}: ({x}, {y}) {c:?}");
                    }
                }
            }
            // The selection keeps its strong look, untouched by the hover.
            for y in rows_of(&d, sel) {
                assert!(!hovered.contains(&y), "{what}: the selected message is not hovered");
                if t.selection != Color::Reset && !gutter(&t) {
                    assert!(
                        (a.left()..a.right()).any(|x| buf[(x, y)].bg == t.selection),
                        "{what}: the selection is drawn on row {y}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_click_turns_the_hover_into_the_selection() {
    let t = resolve("dark", true, Background::Dark);
    for size in SIZES {
        let mut d = open(&t, size);
        let a = area(&d);
        let (i, y) = other(&d);
        moved(&mut d, a.x + 2, y);
        assert!(!hovered_rows(&t, &d.buffer()).is_empty());
        assert!(d.mouse(MouseEventKind::Down(MouseButton::Left), a.x + 2, y));
        d.mouse(MouseEventKind::Up(MouseButton::Left), a.x + 2, y);
        let buf = d.buffer();
        let p = d.pane(PaneKind::Conversation).unwrap();
        assert_eq!(p.range(), Some((i, i)), "{size:?}: the message clicked is selected");
        assert!(hovered_rows(&t, &buf).is_empty(), "{size:?}: and no longer hovered");
        assert!(rows_of(&d, i).iter().any(|&y| (a.left()..a.right()).any(|x| buf[(x, y)].bg == t.selection)));
    }
}

#[test]
fn the_wheel_scrolls_the_view_and_the_hover_follows_what_is_under_the_pointer() {
    let t = resolve("dark", true, Background::Dark);
    for size in SIZES {
        let mut found = false;
        let a = {
            let mut d = Demo::new(size.0, size.1);
            d.open("long-threads");
            d.screen();
            area(&d)
        };
        for y in a.top()..a.bottom() {
            let mut d = Demo::new(size.0, size.1);
            d.app.theme = t.clone();
            d.open("long-threads");
            d.screen();
            let at = |d: &Demo| d.pane(PaneKind::Conversation).unwrap().drawn_rows().into_iter().find(|r| r.1 == y);
            let Some((before, _)) = at(&d) else { continue };
            let sel = d.pane(PaneKind::Conversation).unwrap().selected();
            moved(&mut d, a.x + 3, y);
            assert!(d.mouse(MouseEventKind::ScrollUp, a.x + 3, y), "{size:?}: the wheel draws");
            let buf = d.buffer();
            let Some((after, _)) = at(&d).filter(|(m, _)| *m != before) else { continue };
            found = true;
            assert_eq!(d.pane(PaneKind::Conversation).unwrap().selected(), sel, "{size:?}: the selection stays");
            let hovered = hovered_rows(&t, &buf);
            assert!(hovered.contains(&y), "{size:?}: the message now under the pointer is hovered");
            let rows = rows_of(&d, after);
            assert!(hovered.iter().all(|r| rows.contains(r)), "{size:?}: {hovered:?} of {rows:?}");
            break;
        }
        assert!(found, "{size:?}: some row shows another message after a notch");
    }
}

#[test]
fn a_move_within_one_message_draws_nothing_and_to_another_draws() {
    let t = resolve("dark", true, Background::Dark);
    for size in SIZES {
        let mut d = open(&t, size);
        let a = area(&d);
        let (i, y) = other(&d);
        assert!(moved(&mut d, a.x + 2, y));
        d.screen();
        assert!(!moved(&mut d, a.x + 9, y), "{size:?}: along the same row");
        for r in rows_of(&d, i) {
            assert!(!moved(&mut d, a.x + 4, r), "{size:?}: another row of the same message");
        }
        let sel = d.pane(PaneKind::Conversation).unwrap().selected().unwrap();
        let to = rows_of(&d, sel)[0];
        assert!(moved(&mut d, a.x + 4, to), "{size:?}: onto another message");
        assert!(!moved(&mut d, a.x + 6, to), "{size:?}: and along it");
    }
}

#[test]
fn without_motion_events_a_click_hovers_nothing_until_a_move() {
    let t = resolve("dark", true, Background::Dark);
    for size in SIZES {
        let mut d = open(&t, size);
        let a = area(&d);
        let (i, y) = other(&d);
        d.mouse(MouseEventKind::Down(MouseButton::Left), a.x + 2, y);
        d.mouse(MouseEventKind::Up(MouseButton::Left), a.x + 2, y);
        assert_eq!(d.pane(PaneKind::Conversation).unwrap().selected(), Some(i));
        assert_eq!(d.app.hover_at(), None, "{size:?}: a click says nothing of the pointer");
        // The selection moves away from under the pointer: still nothing is hovered.
        d.keys("j");
        assert_ne!(d.pane(PaneKind::Conversation).unwrap().selected(), Some(i));
        d.mouse(MouseEventKind::ScrollDown, a.x + 2, y);
        assert!(hovered_rows(&t, &d.buffer()).is_empty(), "{size:?}: no motion, no hover");
        let y = rows_of(&d, i).last().copied().expect("still drawn");
        assert!(moved(&mut d, a.x + 2, y));
        assert!(!hovered_rows(&t, &d.buffer()).is_empty(), "{size:?}: a move hovers");
    }
}

#[test]
fn leaving_the_pane_or_losing_focus_clears_the_hover() {
    let t = resolve("dark", true, Background::Dark);
    for size in SIZES {
        let mut d = open(&t, size);
        let a = area(&d);
        let (_, y) = other(&d);
        moved(&mut d, a.x + 2, y);
        assert!(!hovered_rows(&t, &d.buffer()).is_empty());
        // Below the messages: the composer.
        assert!(moved(&mut d, a.x + 2, a.bottom() + 1), "{size:?}: leaving the messages draws");
        assert!(hovered_rows(&t, &d.buffer()).is_empty(), "{size:?}: nothing hovered off the messages");
        moved(&mut d, a.x + 2, y);
        assert!(!hovered_rows(&t, &d.buffer()).is_empty());
        assert!(d.app.handle_event(Event::FocusLost, d.now), "{size:?}: the lost focus draws");
        assert!(hovered_rows(&t, &d.buffer()).is_empty(), "{size:?}: nothing hovered without the focus");
        assert_eq!(d.app.hover_at(), None);
        assert!(!d.app.handle_event(Event::FocusLost, d.now), "{size:?}: nothing more to take away");
        assert!(!d.app.handle_event(Event::FocusGained, d.now), "{size:?}: the focus back hovers nothing");
    }
}

#[test]
fn no_hover_is_drawn_behind_a_popup() {
    let t = resolve("dark", true, Background::Dark);
    let mut d = open(&t, (120, 40));
    let a = area(&d);
    let (_, y) = other(&d);
    moved(&mut d, a.x + 2, y);
    d.keys(":");
    assert_eq!(d.app.hover_at(), None);
    assert!(hovered_rows(&t, &d.buffer()).is_empty(), "the palette is up: nothing hovered under it");
    assert!(!moved(&mut d, a.x + 3, y), "a move under the palette draws nothing");
}
