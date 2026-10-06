//! The shell over the demo world, driven headless: keys and mouse events in, frames out. The
//! demo backend is pumped by hand, the way the binary's loop does it. Screens are insta
//! snapshots (English only); under CI insta never rewrites them.

#[path = "support/demo.rs"]
mod demo;

use demo::{Demo, assert_harmless, mask_hangul};
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use slakio_core::backend::{Backend, Envelope, Event as BackendEvent, Generation};
use slakio_core::i18n::Lang;
use slakio_tui::app::shell::View;
use slakio_tui::app::{App, Focus, Settings};
use slakio_tui::demo::DemoBackend;
use slakio_tui::theme::Theme;
use slakio_world::World;
use std::time::Instant;

#[test]
fn the_demo_opens_on_home_of_the_first_workspace_at_three_sizes() {
    for (w, h) in [(80, 24), (120, 40), (200, 50)] {
        let d = Demo::new(w, h);
        let s = d.screen();
        assert!(s.contains("Favorites") && s.contains("# backend"), "{s}");
        // A narrow status line drops hints of least worth before it cuts the name.
        assert!(d.status_line().contains("▌A comp"), "the workspace keeps its name: {}", d.status_line());
        assert!(d.status_line().contains("demo"), "the status line says the data is invented");
        insta::assert_snapshot!(format!("demo_home_{w}x{h}"), mask_hangul(&s));
    }
}

#[test]
fn the_list_says_loading_until_the_backend_answers() {
    let mut app = App::new(Lang::En, Theme::terminal());
    app.resize(80, 24);
    let backend = DemoBackend::new(World::demo());
    app.connect(backend.capabilities());
    let d = Demo { app, backend, now: Instant::now() };
    assert!(d.screen().contains("Loading"), "{}", d.screen());
}

#[test]
fn an_answer_to_an_older_boot_request_is_dropped() {
    let mut d = Demo::new(80, 24);
    let stale = Envelope { generation: Generation(0), event: BackendEvent::Booted(Box::default()) };
    assert!(!d.app.on_backend(stale), "nothing changes");
    assert_eq!(d.app.model.workspaces().len(), 2, "the newer answer stays");
    // A second connect asks again; the first answer, arriving late, is dropped too.
    let first = d.app.take_commands();
    assert!(first.is_empty());
    let caps = d.backend.capabilities();
    d.app.connect(caps);
    d.app.connect(caps);
    let asked = d.app.take_commands();
    assert_eq!(asked.len(), 2);
    let old = Envelope { generation: asked[0].0, event: BackendEvent::Booted(Box::default()) };
    assert!(!d.app.on_backend(old));
    assert_eq!(d.app.model.workspaces().len(), 2);
}

#[test]
fn the_focused_rail_expands_over_the_list_or_pushes_it_aside() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+h");
    assert_eq!(d.app.focus(), Focus::Rail);
    let s = d.screen();
    assert!(s.contains("A company") && s.contains("Home") && s.contains("Activity"), "{s}");
    insta::assert_snapshot!("demo_rail_overlay_120x40", mask_hangul(&s));
    let mut push = Demo::with(120, 40, Lang::En, Settings { rail_push: true, ..Settings::default() });
    push.keys("space w h");
    insta::assert_snapshot!("demo_rail_push_120x40", push.snap());
}

#[test]
fn the_rail_switches_workspace_and_view() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+h j enter");
    assert_eq!((d.app.shell.workspace, d.app.focus()), (1, Focus::List));
    assert!(d.status_line().contains("B side"), "{}", d.status_line());
    d.keys("space d");
    assert_eq!(d.app.shell.view, View::Dms);
    let s = d.screen();
    assert!(s.contains(" DMs "), "{s}");
    assert!(!s.contains("# general"), "DMs lists no channels");
    insta::assert_snapshot!("demo_dms_120x40", mask_hangul(&s));
}

#[test]
fn views_of_a_later_version_say_so_and_show_no_invented_rows() {
    let mut d = Demo::new(120, 40);
    for (keys, view) in [("space a", View::Activity), ("space f", View::Files), ("space l", View::Later)] {
        d.keys(keys);
        assert_eq!(d.app.shell.view, view);
        let s = d.screen();
        assert!(s.contains("comes in a") && s.contains("Space h  Home"), "{s}");
        assert!(!s.contains("# "), "{s}");
    }
    d.command("home");
    assert_eq!(d.app.shell.view, View::Home);
}

#[test]
fn enter_opens_a_conversation_in_the_work_area_and_the_breadcrumb() {
    let mut d = Demo::new(120, 40);
    let s = d.screen();
    assert!(s.contains("No conversation open") && s.contains("Enter    Open"), "the keys to start with: {s}");
    d.keys("enter");
    let s = d.screen();
    assert!(s.contains("▌#backend") && s.contains("├─ Message #backend") && s.contains("› Press i to write"), "{s}");
    assert_eq!(d.app.focus(), Focus::Conversation, "the opened conversation has the keyboard");
    assert!(d.status_line().contains("#backend"), "{}", d.status_line());
    insta::assert_snapshot!("demo_open_120x40", mask_hangul(&s));
}

#[test]
fn list_keys_move_jump_and_fold() {
    let mut d = Demo::new(120, 40);
    let rows = d.app.shell.rows(&d.app.model).len();
    d.keys("G");
    assert_eq!(d.app.shell.list_cursor, rows - 1);
    d.keys("g g");
    assert_eq!(d.app.shell.list_cursor, 0);
    d.keys("enter");
    assert!(d.screen().contains("▸ Favorites"), "folded");
    assert_eq!(d.app.shell.rows(&d.app.model).len(), rows - 2);
    // Down past the blank row under the folded section, and up again onto Ops.
    d.keys("down down up");
    assert_eq!(d.app.shell.list_cursor, 2);
    assert!(d.screen().contains("▾ Ops"));
}

#[test]
fn a_leader_sequence_shows_its_keys_until_it_ends() {
    let mut d = Demo::new(120, 40);
    d.keys("space w");
    assert!(d.status_line().contains("Space w"), "{}", d.status_line());
    assert_eq!(d.app.focus(), Focus::List);
    d.keys("h");
    assert_eq!(d.app.focus(), Focus::Rail);
    assert!(!d.status_line().contains("Space w"));
    d.keys("space w l ctrl+l");
    assert_eq!(d.app.focus(), Focus::List, "never onto an empty work area");
    d.keys("enter ctrl+h ctrl+l");
    assert_eq!(d.app.focus(), Focus::Conversation);
    // A sequence nothing is bound to is dropped without doing anything.
    d.keys("space x");
    assert_eq!(d.app.focus(), Focus::Conversation);
    assert!(d.app.keys.pending().is_empty());
}

#[test]
fn the_list_panel_hides_and_comes_back() {
    let mut d = Demo::new(120, 40);
    d.keys("space e");
    assert!(d.app.shell.list_hidden);
    assert!(!d.screen().contains("Favorites"));
    d.command("list");
    assert!(d.screen().contains("Favorites"), "{}", d.screen());
}

#[test]
fn hovering_the_rail_expands_it_and_clicks_select_and_open() {
    let mut d = Demo::new(120, 40);
    assert!(d.mouse(MouseEventKind::Moved, 1, 5), "entering the rail redraws");
    assert!(d.app.shell.rail_expanded());
    assert!(!d.mouse(MouseEventKind::Moved, 2, 5), "moving along the same item does not");
    assert!(d.mouse(MouseEventKind::Moved, 2, 6), "another item is lit");
    assert!(d.mouse(MouseEventKind::Moved, 60, 6));
    assert!(!d.app.shell.rail_expanded());
    // Row 2 of the rail is workspace B (row 0 is the border).
    d.mouse(MouseEventKind::Down(MouseButton::Left), 1, 2);
    assert_eq!(d.app.shell.workspace, 1);
    // The separator does nothing.
    d.mouse(MouseEventKind::Down(MouseButton::Left), 1, 3);
    assert_eq!(d.app.shell.workspace, 1);
    // A list row opens it: row 2 is the first channel under the first section.
    let list = d.app.areas().list.unwrap();
    d.mouse(MouseEventKind::Down(MouseButton::Left), list.x + 3, 2);
    let open = d.app.open_target().expect("opened").clone();
    assert_eq!(d.app.model.target(&open).unwrap().workspace.as_str(), "TDEMOB");
    d.mouse(MouseEventKind::Down(MouseButton::Left), 100, 10);
    assert_eq!(d.app.focus(), Focus::Conversation);
}

#[test]
fn a_terminal_too_small_says_how_much_room_it_needs() {
    let d = Demo::new(40, 8);
    let s = d.screen();
    assert!(s.contains("40×8") && s.contains("50×10"), "{s}");
    for (w, h) in [(0, 0), (1, 1), (49, 30), (120, 9), (50, 10)] {
        let mut d = Demo::new(w, h);
        d.keys("ctrl+h j enter space e");
        let _ = d.screen();
    }
}

#[test]
fn the_korean_ui_translates_the_shell() {
    let d = Demo::with(120, 40, Lang::Ko, Settings::default());
    let s = d.screen();
    // The list title "Home" in Korean, and the demo badge.
    assert!(s.contains("\u{D648}") && d.status_line().contains("\u{B370}\u{BAA8}"), "{s}");
    assert!(!s.contains(" Home "), "{s}");
}

#[test]
fn icons_replace_the_rail_letters() {
    let d = Demo::with(120, 40, Lang::En, Settings { icons: true, ..Settings::default() });
    let s = d.screen();
    assert!(s.contains('\u{F02DC}'), "the Home icon: {s}");
}

#[test]
fn hostile_names_are_drawn_sanitised_in_the_list_title_and_status_line() {
    let mut d = Demo::new(120, 40);
    d.keys("space d");
    let rows = d.app.shell.rows(&d.app.model);
    let at = rows
        .iter()
        .position(|r| match r {
            slakio_tui::app::model::Row::Conversation(i) => {
                d.app.model.conversation(*i).name.unsanitized().contains("Mallory")
            }
            _ => false,
        })
        .expect("the DM with the hostile user");
    for _ in 0..at {
        d.keys("j");
    }
    d.keys("enter");
    let s = d.screen();
    assert_harmless(&s);
    assert!(s.contains("◐ Mallory live"), "the list row, its peer in do not disturb: {s}");
    assert!(s.contains("▌ML @Mallory live"), "the pane title, the initials sanitised too: {s}");
    assert!(d.status_line().contains("@Mallory live"), "{}", d.status_line());
    assert!(!s.contains("owned"), "the title sequence is gone, its text too: {s}");
    // A hostile section name of the other workspace.
    // Home puts the rail cursor on Home; one up is workspace B.
    d.keys("space h ctrl+h k enter");
    assert_eq!(d.app.shell.workspace, 1);
    let s = d.screen();
    assert_harmless(&s);
    assert!(s.contains("▾ Side projects"), "{s}");
}
