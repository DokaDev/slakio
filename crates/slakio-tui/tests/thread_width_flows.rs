//! How wide a thread panel can grow, over the demo world: it opens at two fifths capped at 60
//! columns, but once the user resizes it (`Space w >` or the border dragged with the mouse) it
//! grows until the conversation beside it keeps only the least a pane keeps; that width is
//! saved and comes back, and a layout saved before keeps its cap until its first resize.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "this file uses a part of the harness")]
mod scripted;

use demo::Demo;
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use slakio_core::i18n::Label;
use slakio_core::layout::saved::Saved;
use slakio_tui::app::PaneKind;
use slakio_tui::screen::PANE_MIN_WIDTH;

/// `long-threads` open at `w` x `h` with the first thread open beside it, the keyboard on it.
fn thread(w: u16, h: u16) -> Demo {
    let mut d = Demo::new(w, h);
    d.open("long-threads");
    d.keys("g g enter");
    assert_eq!(d.focused_kind(), Some(PaneKind::Thread));
    d
}

/// The widths of the conversation and its thread panel on screen.
fn widths(d: &Demo) -> (u16, u16) {
    let w = |k| d.pane_area(k).map_or(0, |r| r.width);
    (w(PaneKind::Conversation), w(PaneKind::Thread))
}

/// `Space w >` until the thread panel can grow no more (at most `presses` times); how many
/// presses grew it.
fn widen(d: &mut Demo, presses: usize) -> usize {
    let none = d.app.i18n.label(Label::StatusResizeNone).to_string();
    for n in 0..presses {
        d.keys("space w >");
        if d.status_line().contains(&none) {
            return n;
        }
    }
    panic!("still growing after {presses} presses: {:?}", widths(d));
}

#[test]
fn the_thread_pane_widens_past_the_old_cap_at_120_and_200() {
    for (w, opened, widest) in [(120u16, (54, 37), (24, 67)), (200, (104, 61), (24, 141))] {
        let mut d = thread(w, 40);
        let work = d.app.areas().work.width;
        assert_eq!(widths(&d), opened, "{w} wide: two fifths, at most 60, as before");
        assert!(widen(&mut d, 60) > 0);
        assert_eq!(widths(&d), widest, "{w} wide");
        assert_eq!(widths(&d).0, PANE_MIN_WIDTH, "the conversation keeps its least");
        // The two share one border.
        assert_eq!(widths(&d).0 + widths(&d).1, work + 1);
    }
}

#[test]
fn dragging_the_border_left_widens_the_thread_to_the_main_minimum() {
    let mut d = thread(200, 40);
    let main = d.pane_area(PaneKind::Conversation).unwrap();
    let (seam, row) = (main.right() - 1, main.y + 5);
    d.mouse(MouseEventKind::Down(MouseButton::Left), seam, row);
    d.mouse(MouseEventKind::Drag(MouseButton::Left), 0, row);
    d.mouse(MouseEventKind::Up(MouseButton::Left), 0, row);
    assert_eq!(widths(&d), (24, 141));
    // And back again, as far as the thread panel keeps its least.
    let seam = d.pane_area(PaneKind::Conversation).unwrap().right() - 1;
    d.mouse(MouseEventKind::Down(MouseButton::Left), seam, row);
    d.mouse(MouseEventKind::Drag(MouseButton::Left), 199, row);
    d.mouse(MouseEventKind::Up(MouseButton::Left), 199, row);
    assert_eq!(widths(&d).1, 34 + 1);
}

#[test]
fn a_widened_thread_is_saved_and_comes_back() {
    let mut d = thread(200, 40);
    widen(&mut d, 60);
    let saved = d.app.layout_to_save().expect("a layout to keep");
    let text = saved.to_text();
    assert!(text.contains(" 34 65535 24"), "{text}");
    let mut after = Demo::new(200, 40);
    after.app.restore_layout(Saved::parse(&text).expect("reads back"), after.now);
    assert_eq!(widths(&after), (24, 141));
    assert_eq!(after.app.layout_to_save().map(|s| s.to_text()), Some(text));
}

#[test]
fn an_old_layout_file_s_thread_frees_on_its_first_resize() {
    // A thread panel never resized saves the bounds a file written before had.
    let saved = thread(200, 40).app.layout_to_save().expect("a layout to keep");
    let text = saved.to_text();
    assert!(text.contains(" 34 60 40") && !text.contains(" 34 65535 24"), "{text}");
    let mut d = Demo::new(200, 40);
    d.app.restore_layout(Saved::parse(&text).expect("reads back"), d.now);
    assert_eq!(widths(&d), (104, 61), "capped until resized");
    assert_eq!(d.focused_kind(), Some(PaneKind::Thread));
    d.keys("space w >");
    assert_eq!(widths(&d), (100, 65), "past the old cap at the first press");
    assert!(d.app.layout_to_save().unwrap().to_text().contains(" 34 65535 24"));
    widen(&mut d, 60);
    assert_eq!(widths(&d), (24, 141));
    // Evening the room puts the first share back.
    d.keys("space w =");
    assert_eq!(widths(&d), (104, 61));
    assert!(d.app.layout_to_save().unwrap().to_text().contains(" 34 60 40"));
}
