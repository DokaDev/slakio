//! Keyboard and screen behaviour the owner's first look found wrong, each pinned by a test that
//! failed before its fix: where the focus lands, what Enter and Ctrl+W do, the overlay rail, the
//! selection's look, clipped names, the composer and the thread panel, narrow screens, the
//! mouse wheel.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::crossterm::event::MouseEventKind;
use ratatui::style::Modifier;
use slakio_tui::app::model::Row;
use slakio_tui::app::{Focus, Mode, PaneKind};
use slakio_tui::screen;

fn list_row_of(d: &Demo, name: &str) -> usize {
    d.app
        .list_rows()
        .iter()
        .position(|r| matches!(r, Row::Conversation(i) if d.app.model.conversation(*i).name == name))
        .unwrap_or_else(|| panic!("no row {name}"))
}

#[test]
fn ctrl_w_on_the_main_pane_hands_the_focus_to_the_list_on_its_row() {
    let mut d = Demo::new(120, 40);
    d.open("incidents");
    d.keys("ctrl+w");
    assert!(d.app.open_target().is_none());
    assert_eq!(d.app.focus(), Focus::List, "the focus never stays on an empty work area");
    assert_eq!(d.app.list_cursor(), list_row_of(&d, "incidents"));
    // j now moves the list again.
    let at = d.app.list_cursor();
    d.keys("j");
    assert_ne!(d.app.list_cursor(), at);
}

#[test]
fn closing_the_thread_panel_focuses_the_main_pane_on_its_message() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    d.keys("g g enter");
    assert_eq!(d.focused_kind(), Some(PaneKind::Thread));
    d.keys("ctrl+w");
    let main = d.pane(PaneKind::Conversation).unwrap();
    assert_eq!((d.focused_kind(), main.selected()), (Some(PaneKind::Conversation), Some(0)));
}

#[test]
fn enter_without_a_selected_message_starts_writing() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.keys("enter");
    assert!(d.pane(PaneKind::Thread).is_none(), "no thread opens by surprise");
    assert_eq!(d.app.mode(), Mode::Insert);
}

#[test]
fn the_first_cursor_is_on_the_first_conversation_not_a_header() {
    let d = Demo::new(120, 40);
    let rows = d.app.list_rows();
    assert!(matches!(rows[d.app.list_cursor()], Row::Conversation(_)), "{:?}", rows[d.app.list_cursor()]);
}

#[test]
fn the_list_starts_at_the_top_left_corner_and_its_borders_run_down() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+r");
    let a = d.app.areas();
    let list = a.list.expect("the list panel");
    assert_eq!((list.x, list.y), (0, 0));
    let buf = d.buffer();
    // Focused, the view switcher covers nothing: the list's borders show on every row, joined
    // by the rule under the switcher.
    let rule = screen::list_parts(list).rule.unwrap();
    for y in list.y + 1..list.bottom() - 1 {
        let (l, r) = if y == rule { ("├", "┤") } else { ("│", "│") };
        assert_eq!(buf[(0, y)].symbol(), l, "row {y}");
        assert_eq!(buf[(list.right() - 1, y)].symbol(), r, "row {y}");
    }
}

#[test]
fn no_selection_is_drawn_underlined() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.keys("k");
    // The list has lost the focus to the pane, whose message is selected.
    for buf in [d.buffer(), {
        d.keys("ctrl+h");
        d.buffer()
    }] {
        for (i, c) in buf.content().iter().enumerate() {
            assert!(!c.modifier.contains(Modifier::UNDERLINED), "cell {i} {:?} is underlined", c.symbol());
        }
    }
}

#[test]
fn a_long_name_is_clipped_with_an_ellipsis_apart_from_its_badge() {
    // A group DM's names are the longest in the list.
    let mut d = Demo::new(80, 40);
    d.keys("space d");
    let s = d.screen();
    let row = s.lines().find(|l| l.contains("Jiho Park") && l.contains('…'));
    let row = row.unwrap_or_else(|| panic!("a clipped name ends in …:\n{s}"));
    let after = row.split('…').nth(1).unwrap();
    assert!(after.starts_with(' '), "a space between the name and its badge: {row}");
}

#[test]
fn the_composer_has_no_box_of_its_own() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    let s = d.screen();
    assert!(!s.contains("╭ Message") && !s.contains("│╭"), "no border inside the pane's border:\n{s}");
    assert!(s.contains("├─ Message #backend"), "a divider joined to the pane's border:\n{s}");
}

#[test]
fn the_thread_panel_starts_with_its_message_at_the_top() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    let main = d.pane(PaneKind::Conversation).unwrap();
    let i = main.messages().iter().position(|m| m.thread.is_some_and(|t| t.replies == 6)).expect("a short thread");
    d.app.select_message(i);
    d.keys("enter");
    assert_eq!(d.focused_kind(), Some(PaneKind::Thread));
    let thread = d.pane_area(PaneKind::Thread);
    let thread = thread.expect("the thread panel");
    let buf = d.buffer();
    let text = |y: u16| (thread.x + 1..thread.right() - 1).map(|x| buf[(x, y)].symbol()).collect::<String>();
    // The date line, then the thread's message, right under the title.
    assert!(!text(thread.y + 2).trim().is_empty(), "rows under the title are blank:\n{}", d.screen());
    let s = d.screen();
    assert!(s.contains("6 replies"), "a divider names the replies:\n{s}");
}

#[test]
fn a_thread_on_an_80x24_terminal_leaves_the_main_pane_forty_columns() {
    let mut d = Demo::new(80, 24);
    d.open("long-threads");
    d.keys("g g enter");
    let (main, thread) = (d.pane_area(PaneKind::Conversation), d.pane_area(PaneKind::Thread));
    let main = main.expect("the main pane");
    assert!(main.width >= 40, "main pane {} columns", main.width);
    assert!(thread.is_some_and(|t| t.width >= 34));
}

#[test]
fn tab_moves_the_focus_to_the_next_pane_and_back() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    d.keys("g g enter");
    d.keys("tab");
    assert_eq!(d.app.focus(), Focus::ViewSwitcher, "after the thread panel comes the rail");
    d.keys("tab");
    assert_eq!(d.app.focus(), Focus::List, "then the list");
    d.keys("tab");
    assert_eq!(d.focused_kind(), Some(PaneKind::Conversation));
    d.keys("shift+tab");
    assert_eq!(d.app.focus(), Focus::List);
}

#[test]
fn ctrl_q_quits_from_the_list() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+q");
    assert!(d.app.quit);
}

#[test]
fn page_keys_move_the_list() {
    let mut d = Demo::new(80, 24);
    let at = d.app.list_cursor();
    d.keys("pagedown");
    assert!(d.app.list_cursor() > at + 5, "{} -> {}", at, d.app.list_cursor());
    d.keys("end");
    assert_eq!(d.app.list_cursor(), d.app.list_rows().len() - 1);
}

#[test]
fn the_mouse_wheel_scrolls_the_list_and_the_messages() {
    let mut d = Demo::new(120, 40);
    let list = d.app.areas().list.unwrap();
    d.mouse(MouseEventKind::ScrollDown, list.x + 3, 5);
    assert_eq!(d.app.list_top(), 3);
    d.open("backend");
    let work = d.app.areas().work;
    d.mouse(MouseEventKind::ScrollUp, work.x + 10, 10);
    assert!(d.pane(PaneKind::Conversation).unwrap().selected().is_some(), "the wheel moves through the messages");
}

#[test]
fn going_back_with_no_history_says_which_way() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.keys("ctrl+o");
    assert!(d.status_line().contains("No earlier conversation"), "{}", d.status_line());
}

#[test]
fn a_thread_without_replies_says_so_under_its_message() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    let main = d.pane(PaneKind::Conversation).unwrap();
    let i = main.messages().iter().rposition(|m| m.thread.is_none()).expect("a message without a thread");
    d.app.select_message(i);
    d.keys("enter");
    let s = d.screen();
    assert!(s.contains("No replies yet · i reply"), "{s}");
    // A blank row between the message and it, as between blocks.
    let lines: Vec<&str> = s.lines().collect();
    let at = lines.iter().position(|l| l.contains("No replies yet")).unwrap();
    let col = lines[at][..lines[at].find("No replies yet").unwrap()].chars().count();
    let above: String = lines[at - 1].chars().skip(col).take(14).collect();
    assert!(above.trim().is_empty(), "{s}");
    insta::assert_snapshot!("thread_no_replies_120x40", demo::mask_hangul(&s));
}

#[test]
fn screens_of_the_list_without_the_focus_and_of_writing() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    insta::assert_snapshot!("list_unfocused_120x40", d.snap());
    d.keys("i");
    d.type_text("Hello there");
    let s = d.snap();
    assert!(s.contains("│ › Hello there") && s.contains("INSERT"), "{s}");
    insta::assert_snapshot!("composer_insert_120x40", s);
    let d = Demo::new(80, 40);
    insta::assert_snapshot!("list_badges_clip_80x40", d.snap());
}
