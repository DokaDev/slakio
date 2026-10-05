//! The conversation pane over the demo world, driven headless: the message list, the auto
//! thread panel, VISUAL copy, the composer with its local echo, back/forward, and the budget of
//! rows laid out per frame. Screens are insta snapshots (English only, Hangul masked); under CI
//! insta never rewrites them.

#[path = "support/demo.rs"]
mod demo;

use demo::{Demo, assert_harmless, mask_hangul};
use slakio_tui::app::shell::Region;
use slakio_tui::app::work::Side;
use slakio_tui::app::{Effect, Mode};
use slakio_tui::ui::timeline;

/// "Hello" in Korean, as an IME commits it.
const HELLO: &str = "\u{C548}\u{B155}\u{D558}\u{C138}\u{C694}";

#[test]
fn a_channel_opens_in_place_with_its_messages_and_j_k_move_through_them() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    let main = d.app.work.main.as_ref().expect("open");
    assert!(!main.items.is_empty() && main.complete);
    assert_eq!((d.app.shell.focus, main.selected), (Region::Work, None));
    let n = main.items.len();
    d.keys("k");
    assert_eq!(d.app.work.main.as_ref().unwrap().selected, Some(n - 1), "the first move selects the newest");
    d.keys("k k j");
    assert_eq!(d.app.work.main.as_ref().unwrap().selected, Some(n - 2));
    d.keys("g g");
    assert_eq!(d.app.work.main.as_ref().unwrap().selected, Some(0));
    d.keys("G");
    assert_eq!(d.app.work.main.as_ref().unwrap().selected, Some(n - 1));
    d.keys("g g");
    let s = d.screen();
    assert!(s.contains("── 2026-01-0"), "a date separator above the first message: {s}");
    assert!(d.status_line().contains("#backend"));
}

/// Requirements case 1 (default GUI mode, channel view, tab bar hidden) at three sizes.
#[test]
fn case_1_channel_view_snapshots() {
    for (w, h) in [(80, 24), (120, 40), (200, 50)] {
        let mut d = Demo::new(w, h);
        d.open("backend");
        let s = d.screen();
        assert!(s.contains("Message #backend") && !s.contains("Thread"), "{s}");
        insta::assert_snapshot!(format!("case1_channel_{w}x{h}"), mask_hangul(&s));
    }
}

/// Requirements case 2 (the thread opened in the auto thread panel) at three sizes.
#[test]
fn case_2_thread_panel_snapshots() {
    for (w, h) in [(80, 24), (120, 40), (200, 50)] {
        let mut d = Demo::new(w, h);
        d.open("long-threads");
        d.keys("g g");
        d.keys("enter");
        let s = d.screen();
        assert!(s.contains("⤷ Thread · #long-threads") && s.contains(" Reply "), "{s}");
        assert!(d.status_line().contains("#long-threads › ⤷ Thread"), "{}", d.status_line());
        insta::assert_snapshot!(format!("case2_thread_{w}x{h}"), mask_hangul(&s));
    }
}

#[test]
fn enter_on_a_message_opens_its_thread_and_another_one_replaces_it() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    d.keys("g g enter");
    assert_eq!(d.app.work.side, Side::Thread);
    let first = d.app.work.thread.as_ref().unwrap();
    assert!(first.items.len() > 100, "the 1,200-reply thread, newest page first");
    let first = first.target.clone();
    // Back to the channel, one message down, Enter: the panel shows that thread instead.
    d.keys("ctrl+h j enter");
    let second = d.app.work.thread.as_ref().unwrap().target.clone();
    assert_ne!(first, second);
    assert_eq!(d.app.work.side, Side::Thread);
    // Ctrl+W closes the panel, then the conversation.
    d.keys("ctrl+w");
    assert!(d.app.work.thread.is_none() && d.app.work.side == Side::Main);
    d.keys("ctrl+w");
    assert!(d.app.work.main.is_none());
    assert!(d.screen().contains("Choose a conversation"));
}

#[test]
fn messages_show_threads_reactions_and_edits() {
    let mut d = Demo::new(200, 50);
    d.open("general");
    let items = d.app.work.main.as_ref().unwrap().items.clone();
    let find = |p: &dyn Fn(&slakio_tui::app::pane::Shown) -> bool| items.iter().position(p).expect("one exists");
    // (message, any of these shows it)
    let reactions: &[&str] = &[":+1: ", ":eyes: ", ":tada: ", ":fire: ", ":pray: ", ":white_check_mark: "];
    let cases: [(usize, &[&str]); 3] = [
        (find(&|m| m.thread.is_some()), &[" replies · last ", " reply · last "]),
        (find(&|m| !m.reactions.is_empty()), reactions),
        (find(&|m| m.edited), &["(edited)"]),
    ];
    for (i, any) in cases {
        d.app.work.main.as_mut().unwrap().selected = Some(i);
        let s = d.screen();
        assert!(any.iter().any(|x| s.contains(x)), "message {i}: {s}");
    }
}

#[test]
fn the_hostile_channel_draws_nothing_a_terminal_would_act_on() {
    let mut d = Demo::new(120, 40);
    d.open("hostile-strings");
    d.keys("g g");
    for step in 0..30 {
        d.keys("j");
        if step % 3 != 0 {
            continue;
        }
        assert_harmless(&d.screen());
        let bytes = d.terminal_bytes();
        // The backend writes its own CSI sequences (cursor moves, colours); nothing else: no
        // OSC, DCS, APC, PM, SOS, no bell, no 8-bit C1 controls.
        for bad in [&b"\x1b]"[..], b"\x1bP", b"\x1b_", b"\x1b^", b"\x1bX", b"\x07", b"\x1b[2J", b"\x1b[?1049"] {
            assert!(!bytes.windows(bad.len()).any(|w| w == bad), "{bad:?} reached the terminal");
        }
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.chars().any(|c| ('\u{80}'..='\u{9f}').contains(&c)), "a C1 control reached the terminal");
    }
    // The reaction with a hostile name and the hostile poster, sanitised.
    d.keys("g g");
    let s = d.screen();
    assert!(s.contains(":blink: 2"), "{s}");
    insta::assert_snapshot!("hostile_strings_120x40", mask_hangul(&s));
    d.keys("G");
    insta::assert_snapshot!("hostile_strings_end_120x40", mask_hangul(&d.screen()));
}

#[test]
fn the_composer_takes_korean_ime_text_and_enter_echoes_it_locally() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.keys("i");
    assert_eq!(d.app.mode(), Mode::Insert);
    assert!(d.status_line().contains("INSERT"));
    d.type_text(HELLO);
    let s = d.screen();
    assert!(s.contains(HELLO), "the composed text, unbroken: {s}");
    // The text cursor sits after the text (five wide syllables), so an IME's preedit shows there.
    // `j` and `k` are text here.
    d.type_text(" jk");
    d.keys("ctrl+w");
    d.keys("enter");
    let main = d.app.work.main.as_ref().unwrap();
    let last = main.items.last().unwrap();
    assert!(last.own && last.text.as_str() == format!("{HELLO} "), "{:?}", last.text);
    assert!(main.composer.is_empty());
    assert!(d.status_line().contains("not sent anywhere"), "{}", d.status_line());
    let s = d.screen();
    assert!(s.contains(&format!("Me            {HELLO}")), "the echo is in view: {s}");
    d.keys("esc");
    assert_eq!(d.app.mode(), Mode::Normal);
}

#[test]
fn the_composer_is_multiline_and_a_paste_is_sanitised() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.keys("i");
    d.type_text("one");
    d.keys("ctrl+j");
    d.type_text("two");
    d.keys("alt+enter");
    d.type_text("three");
    let paste = ratatui::crossterm::event::Event::Paste("\x1b]0;evil\x07 four\u{202E}".into());
    d.app.handle_event(paste, d.now);
    let c = &d.app.work.main.as_ref().unwrap().composer;
    assert_eq!(c.text(), "one\ntwo\nthree four");
    let s = d.screen();
    assert!(s.contains("│one") && s.contains("│two") && s.contains("│three four"), "{s}");
    d.keys("enter");
    let last = d.app.work.main.as_ref().unwrap().items.last().unwrap().text.clone();
    assert_eq!(last.as_str(), "one\ntwo\nthree four");
}

#[test]
fn a_reply_in_the_thread_panel_is_echoed_there() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    d.keys("g g enter i");
    d.type_text("ack");
    d.keys("enter esc");
    let thread = d.app.work.thread.as_ref().unwrap();
    assert_eq!(thread.items.last().unwrap().text.as_str(), "ack");
    assert_ne!(d.app.work.main.as_ref().unwrap().items.last().unwrap().text.as_str(), "ack");
}

#[test]
fn hangul_typed_in_normal_mode_moves_like_the_keys_under_it() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    let n = d.app.work.main.as_ref().unwrap().items.len();
    // The jamo on the `k` and `j` keys of the 2-set layout.
    d.type_text("\u{314F}\u{314F}\u{314F}\u{3153}");
    assert_eq!(d.app.work.main.as_ref().unwrap().selected, Some(n - 2));
    assert_eq!(d.app.mode(), Mode::Normal);
    // A syllable is the keys that typed it: U+D558 is `g k`: not a binding, nothing breaks.
    d.type_text("\u{D558}");
    assert!(d.app.keys.pending().is_empty());
}

#[test]
fn visual_selects_a_range_and_y_copies_it_through_the_terminal() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.keys("V");
    assert_eq!(d.app.mode(), Mode::Visual);
    assert!(d.status_line().contains("VISUAL"));
    d.keys("k k");
    assert_eq!(d.app.work.main.as_ref().unwrap().range().map(|(a, b)| b - a), Some(2));
    d.keys("y");
    assert_eq!(d.app.mode(), Mode::Normal, "copying ends VISUAL");
    let effects = d.app.take_effects();
    let [Effect::Copy(text)] = &effects[..] else { panic!("{effects:?}") };
    assert_eq!(text.lines().count(), 3, "{text}");
    assert!(d.status_line().contains("Copied 3 messages"), "{}", d.status_line());
    // One message alone copies its text.
    d.keys("y");
    let effects = d.app.take_effects();
    let [Effect::Copy(one)] = &effects[..] else { panic!("{effects:?}") };
    let main = d.app.work.main.as_ref().unwrap();
    assert_eq!(one, main.items[main.selected.unwrap()].text.as_str());
    // Esc leaves VISUAL without copying.
    d.keys("V esc");
    assert_eq!(d.app.mode(), Mode::Normal);
    assert!(d.app.take_effects().is_empty());
}

#[test]
fn back_and_forward_return_to_the_conversations_before() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.open("incidents");
    let at = |d: &Demo| d.app.model.target(&d.app.work.main.as_ref().unwrap().target).unwrap().name.clone();
    d.keys("ctrl+o");
    assert_eq!(at(&d), "backend");
    d.keys("tab");
    assert_eq!(at(&d), "incidents");
    d.keys("alt+left");
    assert_eq!(at(&d), "backend");
    d.keys("ctrl+i");
    assert_eq!(at(&d), "incidents");
    d.keys("ctrl+i");
    assert!(d.status_line().contains("Nothing more"), "{}", d.status_line());
    d.command("back");
    assert_eq!(at(&d), "backend");
}

#[test]
fn the_10k_channel_and_the_1200_reply_thread_lay_out_at_most_twice_the_visible_rows() {
    let mut d = Demo::new(120, 40);
    d.open("big-history");
    // The rows of messages on screen: each open pane's height inside its border, less the
    // composer (three rows when empty).
    let check = |d: &mut Demo, what: &str| {
        let panes = 1 + u64::from(d.app.work.thread.is_some());
        let visible = panes * u64::from(d.app.areas().work.height - 2 - 3);
        timeline::reset_rows_laid_out();
        let _ = d.screen();
        let rows = timeline::rows_laid_out();
        assert!(rows >= visible / 2, "{what}: the screen is filled ({rows})");
        assert!(rows <= 2 * visible, "{what}: {rows} rows laid out for {visible} visible");
    };
    check(&mut d, "opened");
    d.keys("k");
    for _ in 0..60 {
        d.keys("k");
        check(&mut d, "k");
    }
    d.keys("g g");
    let main = d.app.work.main.as_ref().unwrap();
    assert!(main.complete && main.items.len() == 10_000 && main.selected == Some(0), "gg loads to the oldest");
    check(&mut d, "gg");
    d.keys("G");
    check(&mut d, "G");
    d.open("long-threads");
    d.keys("g g enter");
    check(&mut d, "thread");
    d.keys("g g");
    assert_eq!(d.app.work.thread.as_ref().unwrap().items.len(), 1_201, "the thread's message and its replies");
    check(&mut d, "thread gg");
}
