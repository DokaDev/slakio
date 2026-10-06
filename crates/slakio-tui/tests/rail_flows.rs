//! The rail by keyboard: `Ctrl+R` or `Space r` from anywhere outside text, or `Tab`/`Shift+Tab`
//! round the panels (the rail is a stop; it is expanded only while it has the focus), then
//! `j`/`k`/arrows and `Enter` to pick a workspace or a view, `Esc` to leave. The hint line, the
//! which-key popup and the help say how to get there.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use slakio_tui::app::shell::View;
use slakio_tui::app::{Focus, PaneKind};
use std::time::Duration;

#[test]
fn ctrl_r_and_space_r_reach_the_rail_from_the_list_and_a_pane() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+r");
    assert_eq!(d.app.focus(), Focus::Rail, "from the list");
    assert!(d.app.rail_expanded());
    d.keys("ctrl+r");
    assert_eq!(d.app.focus(), Focus::List, "the same key leaves it");
    d.open("backend");
    assert_eq!(d.focused_kind(), Some(PaneKind::Conversation));
    d.keys("space r");
    assert_eq!(d.app.focus(), Focus::Rail, "from a pane, by the leader key");
    d.keys("esc");
    assert_eq!(d.app.focus(), Focus::List);
    assert!(!d.app.rail_expanded(), "it folds as soon as it loses the focus");
}

#[test]
fn shift_tab_from_the_list_and_tab_from_the_last_panel_land_on_the_rail() {
    let mut d = Demo::new(120, 40);
    d.keys("shift+tab");
    assert_eq!(d.app.focus(), Focus::Rail, "the rail is left of the list");
    d.keys("tab");
    assert_eq!(d.app.focus(), Focus::List);
    assert!(!d.app.rail_expanded(), "expanded only while focused: no flash left behind");
    d.open("long-threads");
    d.keys("g g enter");
    assert_eq!(d.focused_kind(), Some(PaneKind::Thread));
    d.keys("tab");
    assert_eq!(d.app.focus(), Focus::Rail, "after the thread panel comes the rail");
    d.keys("tab tab tab");
    assert_eq!(d.focused_kind(), Some(PaneKind::Thread), "and round again");
    d.keys("f6");
    assert_eq!(d.app.focus(), Focus::Rail, "F6 is Tab");
}

#[test]
fn inside_the_rail_arrows_and_enter_pick_a_workspace_and_a_view() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+r");
    // From the first item, workspace B is one down.
    d.keys("home down enter");
    assert_eq!((d.app.workspace(), d.app.focus()), (1, Focus::List), "workspace B, then the list");
    // The cursor is on workspace B now: two down is DMs.
    d.keys("ctrl+r j j enter");
    assert_eq!(d.app.view(), View::Dms);
    d.keys("ctrl+r k k k k enter");
    assert_eq!(d.app.workspace(), 0, "workspace A again");
}

#[test]
fn the_hint_line_says_how_to_reach_the_rail() {
    let d = Demo::new(120, 40);
    assert!(d.status_line().contains("Ctrl+R rail"), "{}", d.status_line());
    let mut d = Demo::new(120, 40);
    d.open("backend");
    assert!(d.status_line().contains("Ctrl+R rail"), "from a pane too: {}", d.status_line());
    d.keys("ctrl+r");
    let s = d.status_line();
    assert!(s.contains("Enter show") && s.contains("j/k move") && s.contains("Esc back"), "{s}");
}

#[test]
fn which_key_and_the_help_list_the_rail_key() {
    let mut d = Demo::new(120, 40);
    d.keys("space");
    d.now += Duration::from_millis(400);
    d.app.on_tick(d.now);
    assert!(d.screen().contains("r  Rail"), "{}", d.screen());
    d.keys("esc ?");
    d.type_text("/rail");
    let s = d.screen();
    assert!(s.contains("Ctrl+R / Space r"), "{s}");
}
