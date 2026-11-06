//! Resizing panes over the demo world, driven headless: `Space w >` / `<` / `+` / `-` and
//! `:resize` give the active pane room from its neighbour (never under a pane's least), a border
//! dragged with the mouse moves, `Space w =` evens again; neighbours share one border; closing a
//! pane gives the keyboard to the pane that takes its room; a new split's shares need what their
//! sides hold, so a smaller screen shrinks every pane alike; zoom's notice goes with the zoom;
//! two thread panes of one conversation are told apart by their titles.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "this file uses a part of the harness")]
mod scripted;

use demo::Demo;
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use slakio_core::i18n::Label;
use slakio_tui::app::PaneHandle;

/// The panes on screen, in reading order, and where.
fn panes(d: &Demo) -> Vec<(PaneHandle, Rect)> {
    d.app.panes_on_screen().into_iter().map(|(p, r)| (p.handle(), r)).collect()
}

fn widths(d: &Demo) -> Vec<u16> {
    panes(d).into_iter().map(|(_, r)| r.width).collect()
}

fn heights(d: &Demo) -> Vec<u16> {
    panes(d).into_iter().map(|(_, r)| r.height).collect()
}

fn focused(d: &Demo) -> PaneHandle {
    d.app.focused_pane().expect("a pane has the keyboard").handle()
}

fn label(d: &Demo, l: Label) -> String {
    d.app.i18n.label(l).to_string()
}

/// `backend` open and split beside: two panes, the right one with the keyboard.
fn two(w: u16, h: u16) -> Demo {
    let mut d = Demo::new(w, h);
    d.open("backend");
    d.keys("v");
    d
}

#[test]
fn neighbours_share_one_border() {
    let mut d = two(120, 40);
    let work = d.app.areas().work;
    let [(_, a), (_, b)] = panes(&d)[..] else { panic!("two panes") };
    assert_eq!(b.x, a.right() - 1, "the right pane starts on the left one's border");
    assert_eq!(a.width + b.width, work.width + 1);
    let s = d.screen();
    let top = s.lines().find(|l| l.contains('┬')).expect("a joined top border");
    assert_eq!(top.matches("╮╭").count(), 1, "only the list panel stands apart: {top}");
    assert!(s.lines().any(|l| l.contains('┴')), "{s}");
    // Under each other: one row shared, the corners joined.
    d.keys("s");
    let s = d.screen();
    assert!(s.lines().any(|l| l.contains('├') && l.contains('┤')), "{s}");
    let [_, (_, top), (_, bottom)] = panes(&d)[..] else { panic!("three panes") };
    assert_eq!(bottom.y, top.bottom() - 1);
}

#[test]
fn keys_give_the_active_pane_room_from_its_neighbour() {
    let mut d = two(120, 40);
    let before = widths(&d);
    d.keys("space w >");
    let wider = widths(&d);
    assert_eq!(wider[1], before[1] + 4, "{before:?} → {wider:?}");
    assert_eq!(wider.iter().sum::<u16>(), before.iter().sum::<u16>());
    d.keys("space w <");
    d.keys("space w <");
    assert_eq!(widths(&d)[1], before[1] - 4);
    // As far as the other keeps its least, then it says so.
    for _ in 0..40 {
        d.keys("space w >");
    }
    let most = widths(&d);
    assert!(most[0] >= 24 && most[0] <= 25, "{most:?}");
    assert!(d.status_line().contains(&label(&d, Label::StatusResizeNone)), "{}", d.status_line());
    // No split under it: no taller.
    d.keys("space w +");
    assert_eq!(widths(&d), most);
    assert!(d.status_line().contains(&label(&d, Label::StatusResizeNone)));
    // Under each other: taller and shorter; a resize done drops the notice.
    d.keys("s");
    let before = heights(&d);
    d.keys("space w -");
    let after = heights(&d);
    assert_eq!(after[2], before[2] - 2, "{before:?} → {after:?}");
    assert!(!d.status_line().contains(&label(&d, Label::StatusResizeNone)), "{}", d.status_line());
    d.keys("space w +");
    assert_eq!(heights(&d), before);
    // Even again.
    d.keys("space w =");
    let even = widths(&d);
    assert!(even[0].abs_diff(even[1]) <= 1, "{even:?}");
}

#[test]
fn resize_as_a_command_takes_the_cells() {
    let mut d = two(120, 40);
    let before = widths(&d);
    d.command("resize >10");
    assert_eq!(widths(&d)[1], before[1] + 10);
    d.command("resize <6");
    assert_eq!(widths(&d)[1], before[1] + 4);
    d.keys("s");
    let h = heights(&d);
    d.command("resize +3");
    assert_eq!(heights(&d)[2], h[2] + 3);
    d.command("resize -3");
    assert_eq!(heights(&d), h);
    // A word it does not take: it says how, and nothing changes.
    d.keys(":");
    d.type_text("resize wide");
    d.keys("enter");
    assert!(d.screen().contains("Resize with +N, -N, >N or <N"), "{}", d.screen());
    d.keys("esc");
    assert_eq!(heights(&d), h);
    // One pane, or none: nothing to resize.
    let mut d = Demo::new(120, 40);
    d.keys("space w >");
    assert!(d.status_line().contains(&label(&d, Label::StatusNoTab)), "{}", d.status_line());
    d.open("backend");
    d.keys("space w >");
    assert!(d.status_line().contains(&label(&d, Label::StatusResizeNone)), "{}", d.status_line());
}

#[test]
fn a_border_dragged_with_the_mouse_moves() {
    let mut d = two(120, 40);
    let [(left, a), (right, b)] = panes(&d)[..] else { panic!("two panes") };
    let seam = a.right() - 1;
    let row = a.y + 5;
    d.mouse(MouseEventKind::Down(MouseButton::Left), seam, row);
    d.mouse(MouseEventKind::Drag(MouseButton::Left), seam - 10, row);
    d.mouse(MouseEventKind::Drag(MouseButton::Left), seam - 12, row);
    d.mouse(MouseEventKind::Up(MouseButton::Left), seam - 12, row);
    let [(_, a2), (_, b2)] = panes(&d)[..] else { panic!("two panes") };
    assert_eq!(a2.right() - 1, seam - 12, "the border is where it was let go");
    assert_eq!(b2.width, b.width + 12);
    assert_eq!(focused(&d), right, "a drag focuses nothing");
    // Not past a pane's least.
    d.mouse(MouseEventKind::Down(MouseButton::Left), a2.right() - 1, row);
    d.mouse(MouseEventKind::Drag(MouseButton::Left), 0, row);
    d.mouse(MouseEventKind::Up(MouseButton::Left), 0, row);
    assert!(widths(&d)[0] >= 24, "{:?}", widths(&d));
    // A press on the border that does not move it is a click on the pane after it.
    d.keys("space w h");
    assert_eq!(focused(&d), left);
    d.now += std::time::Duration::from_secs(1);
    let seam = panes(&d)[0].1.right() - 1;
    d.mouse(MouseEventKind::Down(MouseButton::Left), seam, row);
    d.mouse(MouseEventKind::Up(MouseButton::Left), seam, row);
    assert_eq!(focused(&d), right);
    // Under each other: the row between them drags up and down.
    d.keys("s");
    let [_, (_, top), _] = panes(&d)[..] else { panic!("three panes") };
    let line = top.bottom() - 1;
    d.now += std::time::Duration::from_secs(1);
    d.mouse(MouseEventKind::Down(MouseButton::Left), top.x + 5, line);
    d.mouse(MouseEventKind::Drag(MouseButton::Left), top.x + 5, line - 4);
    d.mouse(MouseEventKind::Up(MouseButton::Left), top.x + 5, line - 4);
    assert_eq!(panes(&d)[1].1.bottom() - 1, line - 4);
}

#[test]
fn closing_a_pane_gives_the_keyboard_to_the_one_that_takes_its_room() {
    // 1 | (2 / 3): closing 2, the keyboard goes to 3 under it, not 1 before it.
    let mut d = two(120, 40);
    let [(_, _), (two_, _)] = panes(&d)[..] else { panic!("two panes") };
    d.keys("s");
    let three_ = focused(&d);
    d.keys("ctrl+k");
    assert_eq!(focused(&d), two_);
    d.keys("ctrl+w");
    assert_eq!(focused(&d), three_);
}

#[test]
fn a_smaller_screen_shrinks_every_pane_alike_before_one_gives_way() {
    // `v` twice: the outer split needs two panes' room on its right.
    let mut d = Demo::new(200, 40);
    d.open("backend");
    d.keys("v");
    d.keys("v");
    assert_eq!(panes(&d).len(), 3);
    // Room for three panes of 24 (and their shared borders): all three stay.
    d.app.resize(36 + 2 + 3 * 24, 40);
    let w = widths(&d);
    assert_eq!(w.len(), 3, "{w:?}");
    assert!(w.iter().all(|w| *w >= 24), "{w:?}");
}

#[test]
fn the_zoomed_notice_goes_with_the_zoom() {
    let mut d = two(120, 40);
    let zoomed = "Zoomed: z shows every pane again";
    d.keys("z");
    assert!(d.status_line().contains(zoomed), "{}", d.status_line());
    d.keys("z");
    assert!(!d.status_line().contains(zoomed), "{}", d.status_line());
    // Shown again by a move.
    d.keys("z");
    d.keys("space w h");
    assert_eq!(panes(&d).len(), 2);
    assert!(!d.status_line().contains(zoomed), "{}", d.status_line());
}

#[test]
fn opening_from_the_list_into_a_zoomed_pane_keeps_the_zoom() {
    let mut d = two(120, 40);
    // The left pane opens a thread panel, then zooms.
    d.keys("space w h");
    d.keys("k");
    d.keys("enter");
    d.keys("space w h");
    d.keys("z");
    assert!(d.screen().contains("[zoomed]"));
    // The list opens another conversation in it: still zoomed, its panel closed.
    d.open("incidents");
    assert!(d.screen().contains("[zoomed]"), "{}", d.screen());
    assert_eq!(panes(&d).len(), 1);
}

#[test]
fn two_thread_panes_of_one_conversation_say_whose_thread() {
    let mut d = Demo::new(160, 40);
    d.open("long-threads");
    d.keys("k");
    d.keys("enter");
    d.keys("space w h");
    d.keys("k k");
    d.keys("v");
    let s = d.screen();
    let titles: Vec<&str> = s.lines().take(4).collect();
    let threads = s.matches("⤷ ").count();
    assert!(threads >= 2, "{s}");
    assert!(!s.contains("⤷ Thread · #long-threads"), "each says whose thread: {titles:?}");
    insta::assert_snapshot!("resize_two_threads_160x40", d.snap());
}

#[test]
fn a_press_where_borders_meet_drags_the_border_that_runs_across() {
    // One pane over two: the `┬` where the lower ones' border meets the one across.
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.keys("space w s");
    d.keys("space w v");
    let [(_, top), (_, left), (_, right)] = panes(&d)[..] else { panic!("three panes") };
    let (x, y) = (left.right() - 1, top.bottom() - 1);
    assert_eq!(d.screen().lines().nth(usize::from(y)).unwrap().chars().nth(usize::from(x)), Some('┬'));
    d.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    d.mouse(MouseEventKind::Drag(MouseButton::Left), x - 6, y + 4);
    d.mouse(MouseEventKind::Up(MouseButton::Left), x - 6, y + 4);
    let [(_, top2), (_, left2), (_, right2)] = panes(&d)[..] else { panic!("three panes") };
    assert_eq!(top2.height, top.height + 4, "the border across moved");
    assert_eq!((left2.width, right2.width), (left.width, right.width), "the one it meets stayed");
}

#[test]
fn zooms_notice_goes_when_the_backend_ends_the_zoom() {
    use slakio_core::backend::{Envelope, Event};
    let mut s = scripted::Scripted::booted(false, None);
    s.d.list_cursor_on("backend");
    s.keys("enter");
    s.d.list_cursor_on("incidents");
    s.keys("v");
    s.keys("z");
    let zoomed = |s: &scripted::Scripted| s.d.status_line().contains("Zoomed");
    assert!(zoomed(&s), "{}", s.d.status_line());
    // The backend's list no longer has `backend`: its pane closes, one is left, no zoom.
    let mut snapshot = slakio_world::World::demo().snapshot().clone();
    snapshot.conversations.retain(|c| c.name.unsanitized() != "backend");
    let id = s.b.log[0].id;
    s.b.push(Envelope { generation: id, event: Event::Booted(Box::new(snapshot)) });
    s.pump();
    assert_eq!(panes(&s.d).len(), 1);
    assert!(!zoomed(&s), "{}", s.d.status_line());
}
