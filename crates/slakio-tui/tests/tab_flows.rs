//! Tabs over the demo world, driven headless: `t` opens a tab (or focuses what is open), the
//! tab bar shows from two tabs on, the keys and the mouse switch, close, reopen, rename and move
//! tabs, `Ctrl+W` closes pane, then tab, down to an empty work area, and the app never
//! reorders the tabs on its own. Screens are insta snapshots (Hangul masked).

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use slakio_tui::app::{Focus, Mode, Overlay, PaneKind};
use slakio_tui::tabbar::{Hit, Part};

/// The titles of the tabs, in order.
fn titles(d: &Demo) -> Vec<String> {
    d.app.tab_titles()
}

/// The name of the conversation the pane with the keyboard shows.
fn focused_name(d: &Demo) -> String {
    let p = d.app.focused_pane().expect("a pane has the keyboard");
    d.app.model.target(p.target()).unwrap().name.line().as_str().to_string()
}

/// Two tabs: `backend` (opened with Enter) and `incidents` (with `t`), the second shown.
fn two_tabs(w: u16, h: u16) -> Demo {
    let mut d = Demo::new(w, h);
    d.open("backend");
    d.open_in_tab("incidents");
    d
}

/// The column of a part of tab `i` on the bar.
fn column(d: &Demo, i: usize, part: Part) -> u16 {
    let bar = d.app.tab_bar().expect("the tab bar shows");
    bar.pieces.iter().find(|p| p.tab == i && p.part == part).map(|p| p.x).expect("drawn")
}

fn bar_row(d: &Demo) -> u16 {
    d.app.areas().tabs.expect("the tab bar shows").y
}

fn click(d: &mut Demo, button: MouseButton, x: u16, y: u16) {
    d.mouse(MouseEventKind::Down(button), x, y);
    d.mouse(MouseEventKind::Up(button), x, y);
    d.pump();
}

#[test]
fn t_opens_a_tab_and_the_bar_shows_from_two_tabs_on() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    assert_eq!(titles(&d), ["#backend"]);
    assert!(d.app.areas().tabs.is_none(), "one tab: plain GUI mode, no bar");
    d.open_in_tab("incidents");
    assert_eq!(titles(&d), ["#backend", "#incidents"]);
    assert_eq!(d.app.current_tab(), Some(1));
    assert_eq!(focused_name(&d), "incidents", "the new tab has the keyboard");
    assert!(d.app.focused_pane().is_some_and(|p| !p.messages().is_empty()), "and loads its messages");
    let s = d.screen();
    let bar = s.lines().next().unwrap();
    assert!(bar.ends_with(" 1 #backend ×  2 #incidents ×"), "{s}");
    insta::assert_snapshot!("two_tabs_120x40", d.snap());
    let d = two_tabs(80, 24);
    insta::assert_snapshot!("two_tabs_80x24", d.snap());
}

#[test]
fn what_is_open_already_is_focused_never_opened_twice() {
    let mut d = two_tabs(120, 40);
    d.open_in_tab("backend");
    assert_eq!((titles(&d).len(), d.app.current_tab()), (2, Some(0)), "t on an open conversation goes there");
    d.open("incidents");
    assert_eq!(d.app.current_tab(), Some(1), "so does Enter, from the other tab");
    assert_eq!(d.app.open_panes().len(), 1, "the tab shown has its one pane");
    // Enter on something new replaces what the pane of the tab shown shows.
    d.open("general");
    assert_eq!(titles(&d), ["#backend", "#general"]);
}

#[test]
fn tabs_switch_by_keys_round_and_by_number() {
    let mut d = two_tabs(120, 40);
    d.open_in_tab("general");
    assert_eq!(d.app.current_tab(), Some(2));
    d.keys("g t");
    assert_eq!(d.app.current_tab(), Some(0), "round past the last");
    d.keys("g T");
    assert_eq!(d.app.current_tab(), Some(2));
    d.keys("ctrl+pageup ctrl+pageup");
    assert_eq!(d.app.current_tab(), Some(0));
    d.keys("ctrl+pagedown");
    assert_eq!((d.app.current_tab(), focused_name(&d)), (Some(1), "incidents".to_string()));
    d.keys("space 3");
    assert_eq!(d.app.current_tab(), Some(2));
    d.keys("alt+1");
    assert_eq!(d.app.current_tab(), Some(0));
    d.keys("space 9");
    assert_eq!(d.app.current_tab(), Some(0));
    assert!(d.status_line().contains("No tab 9"), "{}", d.status_line());
    // From the list too; the keyboard goes to the tab's pane.
    d.list_cursor_on("random");
    d.keys("space 2");
    assert_eq!((d.app.current_tab(), focused_name(&d)), (Some(1), "incidents".to_string()));
}

#[test]
fn ctrl_w_closes_the_pane_then_the_tab_then_leaves_an_empty_work_area() {
    let mut d = two_tabs(120, 40);
    // A thread panel in the second tab: Ctrl+W closes it first.
    d.keys("k enter");
    assert_eq!(d.focused_kind(), Some(PaneKind::Thread));
    d.keys("ctrl+w");
    assert_eq!((d.app.current_tab(), d.focused_kind()), (Some(1), Some(PaneKind::Conversation)));
    d.keys("ctrl+w");
    assert_eq!(titles(&d), ["#backend"], "its last pane closed the tab");
    assert!(d.app.areas().tabs.is_none(), "one tab left: no bar");
    assert_eq!(focused_name(&d), "backend", "the tab shown next has the keyboard");
    d.keys("ctrl+w");
    assert!(titles(&d).is_empty() && d.app.open_target().is_none(), "the work area is empty");
    assert_eq!(d.app.focus(), Focus::List, "nothing quits: the list");
    assert!(!d.app.quit);
    assert_eq!(d.app.closed_tabs(), 2);
    d.keys("ctrl+w");
    assert_eq!(d.app.focus(), Focus::List, "Ctrl+W with nothing open does nothing");
}

#[test]
fn space_t_u_reopens_closed_tabs_where_they_were_with_their_names() {
    let mut d = two_tabs(120, 40);
    d.open_in_tab("general");
    d.keys("space 2");
    d.command("rename Incident");
    assert_eq!(titles(&d), ["#backend", "Incident", "#general"]);
    d.keys("space t c");
    assert_eq!(titles(&d), ["#backend", "#general"]);
    assert_eq!(d.app.current_tab(), Some(1), "the tab to its right is shown");
    d.keys("space 1 space t c");
    assert_eq!(titles(&d), ["#general"]);
    d.keys("space t u");
    assert_eq!(titles(&d), ["#backend", "#general"], "back where it was");
    d.keys("space t u");
    assert_eq!(titles(&d), ["#backend", "Incident", "#general"], "with its name");
    assert_eq!((d.app.current_tab(), focused_name(&d)), (Some(1), "incidents".to_string()));
    d.keys("space t u");
    assert!(d.status_line().contains("No closed tab to reopen"), "{}", d.status_line());
    // Ctrl+O in an empty work area opens the tab closed last.
    d.keys("ctrl+w ctrl+w ctrl+w");
    assert!(titles(&d).is_empty());
    d.keys("ctrl+o");
    assert_eq!(titles(&d), ["#backend"]);
}

#[test]
fn a_tab_is_renamed_by_keys_command_or_double_click_and_an_empty_name_gives_its_title_back() {
    let mut d = two_tabs(120, 40);
    d.keys("space t r");
    assert_eq!(d.app.overlay(), Some(Overlay::Palette));
    assert_eq!(d.app.cmdline.text(), "rename ");
    d.type_text("Incident room");
    assert!(d.screen().contains(":rename Incident room"), "{}", d.screen());
    d.keys("enter");
    assert_eq!(titles(&d), ["#backend", "Incident room"]);
    assert!(d.screen().lines().next().unwrap().contains("2 Incident room ×"));
    d.keys("space t r");
    assert_eq!(d.app.cmdline.text(), "rename Incident room", "the name to edit");
    d.keys("esc");
    d.command("rename ");
    assert_eq!(titles(&d), ["#backend", "#incidents"], "an empty name: named after what it shows");
    // A double click on a tab shows it and asks for its name.
    let (x, y) = (column(&d, 0, Part::Title), bar_row(&d));
    click(&mut d, MouseButton::Left, x, y);
    click(&mut d, MouseButton::Left, x, y);
    assert_eq!(d.app.current_tab(), Some(0));
    assert_eq!(d.app.cmdline.text(), "rename ");
    d.type_text("Backend");
    d.keys("enter");
    assert_eq!(titles(&d), ["Backend", "#incidents"]);
}

#[test]
fn only_the_user_moves_tabs() {
    let mut d = two_tabs(120, 40);
    d.open_in_tab("general");
    d.keys("space 1");
    d.open_in_tab("random");
    assert_eq!(titles(&d), ["#backend", "#random", "#incidents", "#general"], "a new tab goes after the one shown");
    d.keys("space t l");
    assert_eq!(titles(&d), ["#backend", "#incidents", "#random", "#general"]);
    assert_eq!(d.app.current_tab(), Some(2), "the tab moved stays shown");
    d.keys("space t h space t h");
    assert_eq!(titles(&d), ["#random", "#backend", "#incidents", "#general"]);
    d.keys("space t h");
    assert!(d.status_line().contains("at the end already"), "{}", d.status_line());
    // Switching, opening what is open, closing and reopening keep the order.
    d.keys("g t g t");
    d.open_in_tab("general");
    d.keys("space t c space t u");
    assert_eq!(titles(&d), ["#random", "#backend", "#incidents", "#general"]);
}

#[test]
fn the_mouse_shows_closes_and_drags_tabs() {
    let mut d = two_tabs(120, 40);
    d.open_in_tab("general");
    let y = bar_row(&d);
    let (x, y) = (column(&d, 0, Part::Number), y);
    click(&mut d, MouseButton::Left, x, y);
    assert_eq!((d.app.current_tab(), focused_name(&d)), (Some(0), "backend".to_string()));
    // Its × closes a tab; a middle click anywhere on it too.
    let (x, y) = (column(&d, 1, Part::Close), y);
    click(&mut d, MouseButton::Left, x, y);
    assert_eq!(titles(&d), ["#backend", "#general"]);
    let (x, y) = (column(&d, 1, Part::Title), y);
    click(&mut d, MouseButton::Middle, x, y);
    assert_eq!(titles(&d), ["#backend"]);
    d.keys("space t u space t u");
    assert_eq!(titles(&d), ["#backend", "#incidents", "#general"]);
    // Dragged, a tab follows the mouse along the bar.
    let from = column(&d, 0, Part::Title);
    d.mouse(MouseEventKind::Down(MouseButton::Left), from, y);
    d.mouse(MouseEventKind::Drag(MouseButton::Left), column(&d, 1, Part::Title), y);
    d.mouse(MouseEventKind::Drag(MouseButton::Left), column(&d, 2, Part::Title), y + 3);
    d.mouse(MouseEventKind::Drag(MouseButton::Left), column(&d, 2, Part::Title), y);
    d.mouse(MouseEventKind::Up(MouseButton::Left), column(&d, 2, Part::Title), y);
    assert_eq!(titles(&d), ["#incidents", "#general", "#backend"]);
    assert_eq!(d.app.current_tab(), Some(2), "the dragged tab is shown");
    // A drag that starts outside the bar moves nothing.
    d.mouse(MouseEventKind::Down(MouseButton::Left), 60, 20);
    d.mouse(MouseEventKind::Drag(MouseButton::Left), column(&d, 0, Part::Title), y);
    assert_eq!(titles(&d), ["#incidents", "#general", "#backend"]);
}

#[test]
fn tabs_that_do_not_fit_scroll_with_marks_that_reveal_the_rest() {
    let mut d = Demo::new(80, 24);
    d.open("backend");
    for name in ["incidents", "general", "random", "deploys", "long-threads", "big-history"] {
        d.open_in_tab(name);
    }
    let bar = d.app.tab_bar().unwrap();
    let (x, hidden) = bar.left.expect("tabs left out on the left");
    assert!(bar.right.is_none(), "the last tab is shown");
    let s = d.screen();
    assert!(s.lines().next().unwrap().contains('‹'), "{s}");
    insta::assert_snapshot!("tabs_overflow_80x24", d.snap());
    let (x, y) = (x, bar_row(&d));
    click(&mut d, MouseButton::Left, x, y);
    assert_eq!(d.app.current_tab(), Some(hidden), "the nearest tab left out is shown");
    let bar = d.app.tab_bar().unwrap();
    assert!(bar.pieces.iter().any(|p| p.tab == hidden));
    assert_eq!(bar.hit(bar.right.unwrap().0), Some(Hit::More(bar.right.unwrap().1)));
}

#[test]
fn t_on_a_message_opens_its_thread_in_a_tab_titled_by_its_first_line() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    d.keys("t");
    assert!(d.status_line().contains("Select a message first"), "{}", d.status_line());
    d.now += std::time::Duration::from_secs(4);
    d.app.on_tick(d.now);
    let main = d.pane(PaneKind::Conversation).unwrap();
    let i = main.messages().iter().rposition(|m| m.thread.is_some()).expect("a message with a thread");
    let first = main.messages()[i].text.as_str().lines().next().unwrap().trim().to_string();
    d.app.select_message(i);
    d.keys("t");
    assert_eq!(d.app.tab_titles().len(), 2);
    assert_eq!(d.focused_kind(), Some(PaneKind::Thread));
    assert_eq!(titles(&d)[1], format!("⤷ {first}"), "its first line, from the conversation");
    insta::assert_snapshot!("thread_tab_120x40", demo::mask_hangul(&d.screen()));
    // Enter on the same message in the first tab focuses that tab, not a second pane.
    d.keys("space 1");
    d.app.select_message(i);
    d.keys("enter");
    assert_eq!((d.app.current_tab(), d.focused_kind()), (Some(1), Some(PaneKind::Thread)));
    assert_eq!(d.app.open_panes().len(), 1);
}

#[test]
fn a_tab_not_shown_carries_the_mark_of_its_unread_conversations() {
    let mut d = Demo::new(120, 40);
    let unread = (0..)
        .map(|i| d.app.model.conversation(i))
        .find(|c| c.workspace.as_str() == "TDEMOA" && !c.muted && c.unread > 0 && !c.is_dm())
        .expect("an unread channel")
        .clone();
    let name = unread.name.line().as_str().to_string();
    d.open(&name);
    let bar_text = |d: &Demo| d.screen().lines().next().unwrap().to_string();
    d.open_in_tab("backend");
    let badge = if unread.mentions > 0 { format!("@{}", unread.mentions) } else { "●".to_string() };
    assert!(bar_text(&d).contains(&format!("#{name} {badge} ×")), "{}", bar_text(&d));
    d.keys("space 1");
    assert!(!bar_text(&d).contains(&badge), "the tab shown has none: {}", bar_text(&d));
}

#[test]
fn each_tab_keeps_the_history_of_its_own_pane() {
    let mut d = two_tabs(120, 40);
    d.open("general");
    d.keys("space 1");
    d.open("random");
    d.keys("ctrl+o");
    assert_eq!(focused_name(&d), "backend", "the first tab's pane goes back in its own history");
    d.keys("space 2 ctrl+o");
    assert_eq!(focused_name(&d), "incidents");
}

#[test]
fn switching_tabs_by_any_path_leaves_insert_mode() {
    let mut d = two_tabs(120, 40);
    d.keys("i");
    assert_eq!(d.app.mode(), Mode::Insert);
    let (x, y) = (column(&d, 0, Part::Title), bar_row(&d));
    click(&mut d, MouseButton::Left, x, y);
    assert_eq!((d.app.current_tab(), d.app.mode()), (Some(0), Mode::Normal), "a click on another tab");
    d.keys("space 2 i ctrl+p");
    d.type_text("tabnext");
    d.keys("enter");
    assert_eq!((d.app.current_tab(), d.app.mode()), (Some(0), Mode::Normal), "a command from the palette");
    // Writing in the same pane, the composer keeps Insert mode.
    d.keys("i");
    let (x, y) = (column(&d, 0, Part::Number), bar_row(&d));
    click(&mut d, MouseButton::Left, x, y);
    assert_eq!(d.app.mode(), Mode::Insert, "a click on the tab shown changes nothing");
}

#[test]
fn reopening_a_tab_never_opens_twice_what_is_open_already() {
    let mut d = two_tabs(120, 40);
    d.keys("space t c");
    d.open_in_tab("incidents");
    d.keys("space 1 space t u");
    assert_eq!(titles(&d), ["#backend", "#incidents"], "no second #incidents");
    assert_eq!((d.app.current_tab(), focused_name(&d)), (Some(1), "incidents".to_string()), "it is focused");
    assert_eq!(d.app.closed_tabs(), 0, "the closed tab is used up");
    // A closed tab with a thread panel: what is open elsewhere stays out, the rest comes back.
    d.keys("k enter");
    let thread = d.app.focused_pane().unwrap().target().clone();
    d.keys("space t c");
    d.open_in_tab("incidents");
    d.keys("space t u");
    assert_eq!(titles(&d).len(), 3);
    assert_eq!(d.app.focused_pane().map(|p| p.target().clone()), Some(thread), "the thread, in a pane of its own");
    assert_eq!(d.app.open_panes().len(), 1);
    assert_eq!(d.app.open_panes().iter().filter(|p| p.kind() == PaneKind::Conversation).count(), 0);
}

#[test]
fn the_hint_line_and_which_key_teach_the_new_tab_key() {
    let mut d = Demo::new(120, 40);
    d.list_cursor_on("backend");
    let s = d.status_line();
    assert!(s.contains("l peek · t new tab"), "on a conversation of the list: {s}");
    d.keys("enter");
    d.keys("k");
    let s = d.status_line();
    let (t, v) = (s.find("t new tab"), s.find("V select"));
    assert!(t.is_some() && (v.is_none() || t < v), "a selected message, before V select: {s}");
    d.keys("space t");
    d.now += std::time::Duration::from_millis(400);
    d.app.on_tick(d.now);
    assert!(d.screen().lines().any(|l| l.contains("n  Open in a new tab")), "{}", d.screen());
    d.keys("n");
    assert_eq!(titles(&d).len(), 2, "Space t n opens the selected message's thread in a tab");
}

#[test]
fn colon_q_closes_like_ctrl_w_and_only_colon_qa_quits() {
    let mut d = two_tabs(120, 40);
    d.command("q");
    assert_eq!(titles(&d), ["#backend"], ":q closed the pane, so its tab");
    assert!(!d.app.quit);
    d.list_cursor_on("random");
    d.command("quit");
    assert!(titles(&d).is_empty(), "from the list too: the work area's pane");
    assert_eq!(d.app.focus(), Focus::List);
    d.command("q");
    assert!(!d.app.quit, "nothing open: :q never quits");
    assert!(d.status_line().contains("Nothing to close"), "{}", d.status_line());
    d.command("qa");
    assert!(d.app.quit, ":qa quits");
}
