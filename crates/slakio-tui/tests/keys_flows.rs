//! The keyboard, driven headless over the demo world: where the focus lands, what `Esc` does one
//! step at a time, quitting, the keyboard help, the which-key popup, paging keys, the hint line
//! and the questions the app asks. Screens are insta snapshots (English only, Hangul masked).

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use slakio_core::i18n::Lang;
use slakio_tui::app::dialog::Question;
use slakio_tui::app::model::Row;
use slakio_tui::app::shell::{Region, View};
use slakio_tui::app::work::Side;
use slakio_tui::app::{App, Effect, Mode, Settings, WHICH_KEY_DELAY};
use slakio_tui::theme::Theme;
use std::time::{Duration, Instant};

fn list_row_of(d: &Demo, name: &str) -> usize {
    d.app
        .shell
        .rows(&d.app.model)
        .iter()
        .position(|r| matches!(r, Row::Conversation(i) if d.app.model.conversation(*i).name == name))
        .unwrap_or_else(|| panic!("no row {name}"))
}

fn selected(d: &Demo) -> Option<usize> {
    d.app.work.focused().and_then(|p| p.selected)
}

// --- Esc, one step at a time --------------------------------------------------------------

#[test]
fn esc_steps_out_one_level_at_a_time_and_never_closes_anything() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    d.keys("g g enter");
    assert_eq!(d.app.work.side, Side::Thread);
    // 1. A popup closes first.
    d.keys("?");
    assert!(d.app.help.is_some());
    d.keys("esc");
    assert!(d.app.help.is_none());
    // 2. A pending sequence is dropped.
    d.keys("space esc");
    assert!(d.app.keys.pending().is_empty() && d.app.work.side == Side::Thread);
    // 3. Insert → Normal, the text stays.
    d.keys("i");
    d.type_text("draft");
    d.keys("esc");
    assert_eq!(d.app.mode(), Mode::Normal);
    assert_eq!(d.app.work.focused().unwrap().composer.text(), "draft");
    // 4. VISUAL → Normal, the selection stays.
    d.keys("V k esc");
    assert_eq!(d.app.mode(), Mode::Normal);
    assert!(selected(&d).is_some());
    // 5. The selection goes (back to following the newest).
    d.keys("esc");
    assert_eq!(selected(&d), None);
    // 6. The thread panel → the main pane; the thread stays open.
    d.keys("esc");
    assert_eq!((d.app.work.side, d.app.work.thread.is_some()), (Side::Main, true));
    // 5 again on the main pane, then 7: the main pane → the list, on its conversation.
    d.keys("esc esc");
    assert_eq!(d.app.shell.focus, Region::List);
    assert_eq!(d.app.shell.list_cursor, list_row_of(&d, "long-threads"));
    assert!(d.app.work.main.is_some(), "nothing was closed");
    // 9. The list is the outermost: Esc does nothing.
    d.keys("esc");
    assert_eq!(d.app.shell.focus, Region::List);
    // 8. The rail → the list.
    d.keys("ctrl+h esc");
    assert_eq!(d.app.shell.focus, Region::List);
}

// --- Where the focus lands ------------------------------------------------------------------

#[test]
fn enter_opens_and_moves_l_peeks_and_stays() {
    let mut d = Demo::new(120, 40);
    let at = list_row_of(&d, "incidents");
    d.app.shell.list_cursor = at;
    d.keys("l");
    assert_eq!(d.app.shell.focus, Region::List, "a peek keeps the list");
    assert!(d.app.work.main.is_some());
    d.keys("right k enter");
    assert_eq!(d.app.shell.focus, Region::Work);
    let open = d.app.model.target(&d.app.work.main.as_ref().unwrap().target).unwrap().name.clone();
    assert_eq!(open, "backend");
    assert_eq!(selected(&d), None, "no message selected yet; the hints say what to press");
    assert!(d.status_line().contains("i write") && d.status_line().contains("k messages"), "{}", d.status_line());
}

#[test]
fn enter_on_a_message_moves_to_its_thread_and_the_rail_returns_to_the_first_conversation() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    d.keys("k k enter");
    assert_eq!((d.app.work.side, d.app.shell.focus), (Side::Thread, Region::Work));
    assert_eq!(selected(&d), None, "the thread starts with nothing selected");
    d.keys("ctrl+h ctrl+h ctrl+h");
    assert_eq!(d.app.shell.focus, Region::Rail);
    d.keys("j enter");
    assert_eq!((d.app.shell.workspace, d.app.shell.focus), (1, Region::List));
    let rows = d.app.shell.rows(&d.app.model);
    assert!(matches!(rows[d.app.shell.list_cursor], Row::Conversation(_)), "the first conversation, not a header");
}

#[test]
fn h_and_l_and_arrows_move_between_neighbours_where_nothing_moves_sideways() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    d.keys("g g enter");
    d.keys("h");
    assert_eq!((d.app.shell.focus, d.app.work.side), (Region::Work, Side::Main));
    d.keys("right");
    assert_eq!(d.app.work.side, Side::Thread);
    d.keys("left left");
    assert_eq!(d.app.shell.focus, Region::List);
    // In the list: h goes to the section's header, folds it, then the rail.
    d.keys("h");
    assert!(matches!(d.app.shell.rows(&d.app.model)[d.app.shell.list_cursor], Row::Section(_)));
    d.keys("h");
    assert!(d.screen().contains("▸ Ops"), "folded");
    d.keys("h");
    assert_eq!(d.app.shell.focus, Region::Rail);
    d.keys("l");
    assert_eq!(d.app.shell.focus, Region::List);
    d.keys("l");
    assert!(d.screen().contains("▾ Ops"), "l unfolds");
    // { and } jump between section headers.
    d.keys("}");
    let header = |d: &Demo| matches!(d.app.shell.rows(&d.app.model)[d.app.shell.list_cursor], Row::Section(_));
    assert!(header(&d));
    let at = d.app.shell.list_cursor;
    d.keys("{");
    assert!(header(&d) && d.app.shell.list_cursor < at);
}

#[test]
fn tab_and_f6_go_round_the_open_panels_skipping_a_hidden_list() {
    let mut d = Demo::new(120, 40);
    d.keys("tab");
    assert_eq!(d.app.shell.focus, Region::Rail, "nothing open: the rail and the list are the stops");
    d.keys("tab");
    assert_eq!(d.app.shell.focus, Region::List);
    d.open("long-threads");
    d.keys("g g enter f6");
    assert_eq!(d.app.shell.focus, Region::Rail);
    d.keys("f6");
    assert_eq!(d.app.shell.focus, Region::List);
    d.keys("shift+f6");
    assert_eq!(d.app.shell.focus, Region::Rail, "backwards, the rail is before the list");
    d.keys("shift+f6");
    assert_eq!((d.app.shell.focus, d.app.work.side), (Region::Work, Side::Thread));
    d.keys("space e tab");
    assert_eq!(d.app.shell.focus, Region::Rail);
    d.keys("tab");
    assert_eq!((d.app.shell.focus, d.app.work.side), (Region::Work, Side::Main), "the hidden list is skipped");
}

#[test]
fn ctrl_j_and_ctrl_k_say_there_is_no_pane_that_way() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+j");
    assert!(d.status_line().contains("No pane below"), "{}", d.status_line());
    d.keys("space w k");
    assert!(d.status_line().contains("No pane above"), "{}", d.status_line());
}

#[test]
fn space_w_c_closes_like_ctrl_w_and_space_brackets_go_back_and_forward() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.open("incidents");
    d.keys("space [");
    let name = |d: &Demo| d.app.model.target(&d.app.work.main.as_ref().unwrap().target).unwrap().name.clone();
    assert_eq!(name(&d), "backend");
    d.keys("space ]");
    assert_eq!(name(&d), "incidents");
    d.keys("space w c");
    assert!(d.app.work.main.is_none());
    assert_eq!(d.app.shell.focus, Region::List);
}

#[test]
fn space_capital_w_picks_a_workspace_on_the_rail() {
    let mut d = Demo::new(120, 40);
    d.keys("space W");
    assert_eq!((d.app.shell.focus, d.app.shell.rail_cursor), (Region::Rail, 0));
    d.keys("j enter");
    assert_eq!(d.app.shell.workspace, 1);
    d.command("workspace");
    assert_eq!((d.app.shell.focus, d.app.shell.rail_cursor), (Region::Rail, 1));
}

// --- Arrows, pages, Home and End wherever j and k work --------------------------------------

#[test]
fn arrows_pages_home_and_end_work_wherever_j_and_k_do() {
    let mut d = Demo::new(120, 40);
    // The rail.
    d.keys("ctrl+h end");
    assert_eq!(d.app.shell.rail_cursor, 6);
    d.keys("home down");
    assert_eq!(d.app.shell.rail_cursor, 1);
    d.keys("esc");
    // The list.
    let at = d.app.shell.list_cursor;
    d.keys("ctrl+d");
    let half = d.app.shell.list_cursor;
    assert!(half > at, "half a page down");
    d.keys("pageup");
    assert!(d.app.shell.list_cursor < half);
    // A pane.
    d.open("big-history");
    d.keys("end");
    let n = d.app.work.main.as_ref().unwrap().items.len();
    assert_eq!(selected(&d), Some(n - 1));
    d.keys("pageup");
    let page = n - 1 - selected(&d).unwrap();
    assert!(page > 10, "a page of messages: {page}");
    d.keys("ctrl+d");
    assert!(n - 1 - selected(&d).unwrap() < page);
    d.keys("up down");
    assert!(selected(&d).is_some());
    d.keys("home");
    assert_eq!(selected(&d), Some(0));
}

// --- Quitting ---------------------------------------------------------------------------------

#[test]
fn ctrl_q_quits_everywhere_also_while_typing() {
    for keys in ["ctrl+q", "space q", ": q enter", "i ctrl+q", "? ctrl+q", ": ctrl+q"] {
        let mut d = Demo::new(120, 40);
        d.open("backend");
        d.keys(keys);
        assert!(d.app.quit, "{keys}");
    }
    let mut d = Demo::new(120, 40);
    d.keys("q");
    assert!(!d.app.quit, "q alone never quits");
}

#[test]
fn quitting_with_a_message_not_sent_asks_first_and_enter_stays() {
    let mut d = Demo::new(80, 24);
    d.open("backend");
    d.keys("i");
    d.type_text("half written");
    d.keys("ctrl+q");
    assert!(!d.app.quit);
    assert_eq!(d.app.dialog.map(|q| q.question), Some(Question::Quit));
    let s = d.snap();
    assert!(s.contains("Quit slakio?") && s.contains("[ Stay ]"), "{s}");
    insta::assert_snapshot!("quit_confirm_80x24", s);
    d.keys("ctrl+q");
    assert!(!d.app.quit, "asked already: the question stays");
    d.keys("enter");
    assert!(!d.app.quit && d.app.dialog.is_none(), "Enter keeps the safe answer");
    assert_eq!(d.app.work.main.as_ref().unwrap().composer.text(), "half written");
    d.keys("ctrl+q n");
    assert!(!d.app.quit);
    d.keys("esc space q y");
    assert!(d.app.quit);
}

#[test]
fn ctrl_c_cancels_and_says_how_to_quit() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+c");
    assert!(!d.app.quit);
    assert!(d.status_line().contains("Press Ctrl+Q to quit"), "{}", d.status_line());
    d.keys("space ctrl+c");
    assert!(d.app.keys.pending().is_empty());
    d.open("backend");
    d.keys("i ctrl+c");
    assert_eq!(d.app.mode(), Mode::Normal, "Ctrl+C leaves Insert as in vim");
    d.keys(": ctrl+c");
    assert_eq!(d.app.mode(), Mode::Normal);
}

// --- The which-key popup --------------------------------------------------------------------

#[test]
fn the_which_key_popup_shows_after_a_short_wait_and_not_before() {
    let mut d = Demo::new(120, 40);
    let t0 = d.now;
    d.keys("space");
    assert_eq!(d.app.deadline(), Some(t0 + WHICH_KEY_DELAY), "the loop wakes up for it, once");
    d.now = t0 + WHICH_KEY_DELAY - Duration::from_millis(1);
    assert!(!d.screen().contains("Space — Leader"));
    assert!(d.status_line().contains("Space …"), "{}", d.status_line());
    d.now = t0 + WHICH_KEY_DELAY;
    assert!(d.app.on_tick(d.now), "the popup's time is a redraw");
    assert_eq!(d.app.deadline(), None, "then nothing to wake up for");
    let s = d.snap();
    assert!(s.contains("Space — Leader") && s.contains("+Window") && s.contains("Esc close"), "{s}");
    insta::assert_snapshot!("whichkey_space_120x40", s);
    // Space w: the group.
    d.keys("w");
    let s = d.snap();
    assert!(s.contains("Space w — Window") && s.contains("Close the pane"), "{s}");
    insta::assert_snapshot!("whichkey_space_w_120x40", s);
    // Backspace goes back up a level; Esc closes.
    d.keys("backspace");
    assert!(d.snap().contains("Space — Leader"));
    d.keys("esc");
    assert!(!d.snap().contains("Space — Leader") && d.app.keys.pending().is_empty());
    // Typed quickly, a sequence runs without the popup.
    d.now = t0;
    d.keys("space d");
    assert_eq!(d.app.shell.view, View::Dms);
    assert!(!d.screen().contains("Leader"));
}

#[test]
fn the_which_key_popup_fits_an_80x24_terminal_and_question_mark_opens_the_help() {
    let mut d = Demo::new(80, 24);
    let t0 = d.now;
    d.keys("space");
    d.now = t0 + WHICH_KEY_DELAY;
    d.app.on_tick(d.now);
    insta::assert_snapshot!("whichkey_space_80x24", d.snap());
    d.keys("w ?");
    assert!(d.app.help.is_some(), "? after a group shows every key");
}

// --- The keyboard help ----------------------------------------------------------------------

#[test]
fn question_mark_f1_and_space_question_mark_open_the_help_where_the_keyboard_is() {
    let mut d = Demo::new(120, 40);
    for keys in ["?", "f1", "space ?"] {
        d.keys(keys);
        let h = d.app.help.as_ref().expect(keys);
        assert_eq!(h.origin, slakio_tui::keymap::Ctx::List);
        d.keys("esc");
        assert!(d.app.help.is_none(), "{keys}");
    }
    d.keys("?");
    let s = d.snap();
    assert!(s.contains("Keys — List panel") && s.contains("▾ List panel"), "{s}");
    let rows = d.app.help.as_ref().unwrap().rows(&d.app.keymap, &d.app.i18n);
    let rail = rows.iter().any(|r| {
        matches!(r, slakio_tui::app::help::Row::Section { ctx: slakio_tui::keymap::Ctx::Rail, open: false, count } if *count > 0)
    });
    assert!(rail, "other contexts are folded with their number of keys");
    insta::assert_snapshot!("help_list_120x40", s);
    // F1 works while typing, and toggles.
    d.keys("f1");
    d.open("backend");
    d.keys("i f1");
    assert!(d.app.help.as_ref().is_some_and(|h| h.origin == slakio_tui::keymap::Ctx::ComposerInsert));
    d.keys("f1");
    assert!(d.app.help.is_none());
    assert_eq!(d.app.mode(), Mode::Insert, "back to typing");
}

#[test]
fn the_help_moves_searches_and_runs_the_key_under_the_cursor() {
    let mut d = Demo::new(80, 24);
    d.keys("? / s h o w space d");
    let s = d.snap();
    assert!(s.contains("show d") && s.contains("Show DMs"), "{s}");
    insta::assert_snapshot!("help_search_80x24", s);
    d.keys("enter");
    assert!(!d.app.help.as_ref().unwrap().typing, "Enter ends typing");
    // The rows: the section, then Show DMs. Enter runs it and closes the help.
    d.keys("j enter");
    assert!(d.app.help.is_none());
    assert_eq!(d.app.shell.view, View::Dms);
    // A section opens and closes with Enter, h and l.
    d.keys("? G");
    let rows = d.app.help.as_ref().unwrap().rows(&d.app.keymap, &d.app.i18n).len();
    d.keys("enter");
    let more = d.app.help.as_ref().unwrap().rows(&d.app.keymap, &d.app.i18n).len();
    assert!(more > rows, "the last section opened");
    d.keys("h");
    assert_eq!(d.app.help.as_ref().unwrap().rows(&d.app.keymap, &d.app.i18n).len(), rows);
    d.keys("q");
    assert!(d.app.help.is_none());
}

// --- The hint line --------------------------------------------------------------------------

#[test]
fn the_hint_line_fits_where_the_keyboard_is_and_its_keys_work() {
    let mut d = Demo::new(160, 40);
    let hints = |d: &Demo| d.status_line();
    assert!(
        hints(&d).contains("Enter open · l peek · Tab next pane · : commands · Ctrl+R rail · ? help · Space more"),
        "{}",
        hints(&d)
    );
    d.keys("g g");
    assert!(hints(&d).contains("Enter fold · j/k move"), "{}", hints(&d));
    d.keys("ctrl+h");
    assert!(hints(&d).contains("Enter show · j/k move · Esc back"), "{}", hints(&d));
    d.keys("esc");
    d.open("long-threads");
    assert!(hints(&d).contains("i write · k messages · Esc list · Ctrl+R rail"), "{}", hints(&d));
    d.keys("k");
    assert!(hints(&d).contains("Enter thread · y copy · V select · i write · Esc deselect"), "{}", hints(&d));
    d.keys("V");
    assert!(hints(&d).contains("y copy · j/k extend · Esc cancel"), "{}", hints(&d));
    d.keys("esc enter");
    assert!(hints(&d).contains("i reply · k messages · Esc main · Ctrl+W close"), "{}", hints(&d));
    d.keys("i");
    assert!(hints(&d).contains("Enter send · Alt+Enter newline · Esc stop typing"), "{}", hints(&d));
    d.keys("esc :");
    assert!(hints(&d).contains("Enter run · Esc cancel"), "{}", hints(&d));
    // Narrow: hints of least worth go first, the workspace's name stays; the badge and `demo`
    // stay.
    let mut d = Demo::new(80, 24);
    d.open("backend");
    let line = d.status_line();
    assert!(line.starts_with(" NORMAL  ▌A comp") && line.ends_with("demo"), "{line}");
    assert!(line.contains("i write"), "{line}");
    let welcome = App::new(Lang::En, Theme::terminal());
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 10)).unwrap();
    t.draw(|f| slakio_tui::ui::draw(f, &welcome, Instant::now())).unwrap();
    let last: String = (0..100).map(|x| t.backend().buffer()[(x, 9)].symbol().to_string()).collect();
    assert!(last.contains("Ctrl+Q quit · ? help · : commands"), "{last}");
}

// --- The quick switcher and protected keys -------------------------------------------------

#[test]
fn ctrl_p_opens_the_command_line_from_anywhere() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.keys("i ctrl+p");
    assert_eq!(d.app.mode(), Mode::Command);
    d.type_text("dms");
    d.keys("enter");
    assert_eq!(d.app.shell.view, View::Dms);
}

// --- The icons question --------------------------------------------------------------------

#[test]
fn the_icons_question_previews_the_answer_and_saves_it() {
    let mut d = Demo::with(120, 40, Lang::En, Settings::default());
    d.app.ask_icons();
    assert_eq!(d.app.key_context(), slakio_tui::keymap::Ctx::Dialog);
    let s = d.screen();
    assert!(s.contains("Nerd Font icons?") && s.contains("[ No, letters ]"), "{s}");
    d.keys("right");
    assert!(d.app.settings.icons, "the rail previews the answer with the focus");
    assert!(d.screen().contains('\u{F02DC}'));
    d.keys("left");
    assert!(!d.app.settings.icons);
    d.keys("enter");
    assert_eq!(d.app.take_effects(), [Effect::Save { key: "icons", value: "off".into() }], "Enter: the safe answer");
    d.app.ask_icons();
    d.keys("y");
    assert!(d.app.settings.icons);
    assert_eq!(d.app.take_effects(), [Effect::Save { key: "icons", value: "on".into() }]);
    // Ctrl+Q still quits while it asks.
    d.app.ask_icons();
    d.keys("ctrl+q");
    assert!(d.app.quit);
}

#[test]
fn a_key_event_with_shift_tab_from_a_terminal_is_the_previous_panel() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.app.handle_event(Event::Key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)), d.now);
    assert_eq!(d.app.shell.focus, Region::List);
}

#[test]
fn the_deadline_never_wakes_the_loop_for_nothing() {
    let mut d = Demo::new(120, 40);
    assert_eq!(d.app.deadline(), None);
    d.keys("space h");
    assert_eq!(d.app.deadline(), None, "the sequence ended before the popup's time");
    let t0 = Instant::now();
    d.now = t0;
    d.keys("g");
    let at = d.app.deadline().expect("the popup's time");
    d.app.on_tick(at);
    assert_eq!(d.app.deadline(), None);
}
