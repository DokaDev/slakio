//! The conversation pane over the demo world, driven headless: the message list, the auto
//! thread panel, VISUAL copy, the composer with its local echo, back/forward, and the budget of
//! rows laid out per frame. Screens are insta snapshots (English only, Hangul masked); under CI
//! insta never rewrites them.

#[path = "support/demo.rs"]
mod demo;

use demo::{Demo, assert_harmless, mask_hangul};
use slakio_tui::app::dialog::Question;
use slakio_tui::app::{Effect, Focus, Mode, Overlay, PaneKind, PaneRef};
use slakio_tui::ui::timeline;

/// The conversation pane (open).
fn conversation(d: &Demo) -> PaneRef<'_> {
    d.pane(PaneKind::Conversation).expect("a conversation is open")
}

/// "Hello" in Korean, as an IME commits it.
const HELLO: &str = "\u{C548}\u{B155}\u{D558}\u{C138}\u{C694}";

#[test]
fn a_channel_opens_in_place_with_its_messages_and_j_k_move_through_them() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    let main = d.pane(PaneKind::Conversation).expect("open");
    assert!(!main.messages().is_empty() && main.complete());
    assert_eq!((d.focused_kind(), main.selected()), (Some(PaneKind::Conversation), None));
    let n = main.messages().len();
    d.keys("k");
    assert_eq!(conversation(&d).selected(), Some(n - 1), "the first move selects the newest");
    d.keys("k k j");
    assert_eq!(conversation(&d).selected(), Some(n - 2));
    d.keys("g g");
    assert_eq!(conversation(&d).selected(), Some(0));
    d.keys("G");
    assert_eq!(conversation(&d).selected(), Some(n - 1));
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
        // A narrow status line cuts the place in its middle.
        let place = if w >= 120 { "#long-threads › ⤷ Thread" } else { "⤷ Thread" };
        assert!(d.status_line().contains(place), "{}", d.status_line());
        insta::assert_snapshot!(format!("case2_thread_{w}x{h}"), mask_hangul(&s));
    }
}

/// Blank rows between the main pane's top border and its first drawn row.
fn rows_above_the_history(d: &Demo, name: &str) -> usize {
    let screen = d.screen();
    let lines: Vec<&str> = screen.lines().collect();
    let mark = format!("▌#{name} ");
    let title = lines.iter().position(|l| l.contains(&mark)).expect("the pane title");
    // The pane's own mark (the list panel's title has the workspace's).
    let at = lines[title].find(&mark).expect("the title mark");
    let col = lines[title][..at].chars().count() - 1;
    lines[title + 1..].iter().take_while(|l| l.chars().skip(col).take(20).collect::<String>().trim().is_empty()).count()
}

#[test]
fn gg_puts_the_oldest_message_on_the_first_row_and_a_short_history_sits_at_the_bottom() {
    // A history longer than the screen: after `gg` the oldest message (under its date) is on
    // the first row and newer ones fill the rest, cut at the bottom; no gap above it.
    let mut d = Demo::new(80, 24);
    d.open("deploys");
    d.keys("g g");
    assert_eq!(rows_above_the_history(&d, "deploys"), 0, "{}", d.screen());
    assert!(d.screen().contains("── 2026-01-0"), "the date of the oldest message");
    insta::assert_snapshot!("gg_long_history_80x24", mask_hangul(&d.screen()));
    d.keys("j j j k k k");
    assert_eq!(rows_above_the_history(&d, "deploys"), 0, "moving near the top keeps it there");
    // A history that fits whole sits at the bottom, by the composer, before and after `gg` (in
    // the compact layout twenty messages fit on a tall screen).
    let mut d = Demo::with(
        120,
        60,
        slakio_core::i18n::Lang::En,
        slakio_tui::app::Settings { compact: true, ..Default::default() },
    );
    d.open("feed-ticket-104");
    let main = conversation(&d);
    assert!(main.complete() && main.messages().len() == 20, "a short channel: {}", main.messages().len());
    let gap = rows_above_the_history(&d, "feed-ticket-104");
    assert!(gap > 0, "it fits with rows to spare");
    d.keys("g g");
    assert_eq!(rows_above_the_history(&d, "feed-ticket-104"), gap, "gg moves nothing");
    insta::assert_snapshot!("gg_short_history_120x60", mask_hangul(&d.screen()));
}

#[test]
fn enter_on_a_message_opens_its_thread_and_another_one_replaces_it() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    d.keys("g g enter");
    assert_eq!(d.focused_kind(), Some(PaneKind::Thread));
    let first = d.pane(PaneKind::Thread).unwrap();
    assert!(first.messages().len() > 100, "the 1,200-reply thread, newest page first");
    let first = first.target().clone();
    // Back to the channel, one message down, Enter: the panel shows that thread instead.
    d.keys("ctrl+h j enter");
    let second = d.pane(PaneKind::Thread).unwrap().target().clone();
    assert_ne!(first, second);
    assert_eq!(d.focused_kind(), Some(PaneKind::Thread));
    // Ctrl+W closes the panel, then the conversation.
    d.keys("ctrl+w");
    assert!(d.pane(PaneKind::Thread).is_none() && d.focused_kind() == Some(PaneKind::Conversation));
    d.keys("ctrl+w");
    assert!(d.app.open_target().is_none());
    assert_eq!(d.app.focus(), Focus::List, "the list has the keyboard again");
    assert!(d.screen().contains("No conversation open"));
}

#[test]
fn messages_show_threads_reactions_and_edits() {
    let mut d = Demo::new(200, 50);
    d.open("general");
    let items = conversation(&d).messages().to_vec();
    let find = |p: &dyn Fn(&slakio_tui::app::pane::Shown) -> bool| items.iter().position(p).expect("one exists");
    // (message, any of these shows it)
    // Reactions are chips of emoji: ` 👍 2 `.
    let emoji: Vec<String> = ["+1", "eyes", "tada", "fire", "pray", "white_check_mark"]
        .iter()
        .map(|n| format!(" {} ", slakio_tui::emoji::get(n).unwrap()))
        .collect();
    let reactions: Vec<&str> = emoji.iter().map(String::as_str).collect();
    let reactions: &[&str] = &reactions;
    let cases: [(usize, &[&str]); 3] = [
        (find(&|m| m.thread.is_some()), &[" replies · last ", " reply · last "]),
        (find(&|m| !m.reactions.is_empty()), reactions),
        (find(&|m| m.edited), &["(edited)"]),
    ];
    for (i, any) in cases {
        d.app.select_message(i);
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
    let main = conversation(&d);
    let last = main.messages().last().unwrap();
    assert!(last.own && last.text.as_str() == format!("{HELLO} "), "{:?}", last.text);
    assert!(main.composer_text().is_empty());
    assert!(d.status_line().contains("not sent anywhere"), "{}", d.status_line());
    let s = d.screen();
    assert!(s.contains("Me  ") && s.contains(HELLO), "the echo is in view, under the user's name: {s}");
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
    assert_eq!(conversation(&d).composer_text(), "one\ntwo\nthree four");
    let s = d.screen();
    assert!(s.contains("│ › one") && s.contains("│   two") && s.contains("│   three four"), "{s}");
    d.keys("enter");
    let last = conversation(&d).messages().last().unwrap().text.clone();
    assert_eq!(last.as_str(), "one\ntwo\nthree four");
}

#[test]
fn a_reply_in_the_thread_panel_is_echoed_there() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    d.keys("g g enter i");
    d.type_text("ack");
    d.keys("enter esc");
    let thread = d.pane(PaneKind::Thread).unwrap();
    assert_eq!(thread.messages().last().unwrap().text.as_str(), "ack");
    assert_ne!(conversation(&d).messages().last().unwrap().text.as_str(), "ack");
}

#[test]
fn hangul_typed_in_normal_mode_moves_like_the_keys_under_it() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    let n = conversation(&d).messages().len();
    // The jamo on the `k` and `j` keys of the 2-set layout.
    d.type_text("\u{314F}\u{314F}\u{314F}\u{3153}");
    assert_eq!(conversation(&d).selected(), Some(n - 2));
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
    assert_eq!(conversation(&d).range().map(|(a, b)| b - a), Some(2));
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
    let main = conversation(&d);
    assert_eq!(one, main.messages()[main.selected().unwrap()].text.as_str());
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
    let at = |d: &Demo| d.app.model.target(d.app.open_target().unwrap()).unwrap().name.clone();
    d.keys("ctrl+o");
    assert_eq!(at(&d), "backend");
    d.keys("space ]");
    assert_eq!(at(&d), "incidents");
    d.keys("alt+left");
    assert_eq!(at(&d), "backend");
    d.keys("alt+right");
    assert_eq!(at(&d), "incidents");
    d.keys("space ]");
    assert!(d.status_line().contains("No later conversation"), "{}", d.status_line());
    d.command("back");
    assert_eq!(at(&d), "backend");
    // `Tab` is the next panel; `Ctrl+I` goes forward only where the terminal tells it from `Tab`
    // (the kitty keyboard protocol).
    d.keys("ctrl+i");
    assert_eq!(at(&d), "backend", "not bound without the protocol (it arrives as Tab)");
    d.app.keymap = slakio_tui::keymap::Keymap::new(true);
    d.keys("ctrl+i");
    assert_eq!(at(&d), "incidents");
    // Closed, the conversation comes back with Back, with its own history.
    d.keys("ctrl+w ctrl+o");
    assert_eq!(at(&d), "incidents", "the one closed last");
    d.keys("ctrl+o");
    assert_eq!(at(&d), "backend");
}

#[test]
fn the_10k_channel_and_the_1200_reply_thread_lay_out_at_most_twice_the_visible_rows() {
    let mut d = Demo::new(120, 40);
    d.open("big-history");
    // The rows of messages on screen: each shown pane's message area.
    let check = |d: &mut Demo, what: &str| {
        let on_screen = d.app.panes_on_screen();
        let areas = on_screen.iter().filter_map(|(p, _)| d.app.message_area(p.handle()));
        let visible: u64 = areas.map(|r| u64::from(r.height)).sum();
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
    let main = conversation(&d);
    assert!(main.complete() && main.messages().len() == 10_000 && main.selected() == Some(0), "gg loads to the oldest");
    check(&mut d, "gg");
    d.keys("G");
    check(&mut d, "G");
    d.open("long-threads");
    d.keys("g g enter");
    check(&mut d, "thread");
    d.keys("g g");
    assert_eq!(d.pane(PaneKind::Thread).unwrap().messages().len(), 1_201, "the thread's message and its replies");
    check(&mut d, "thread gg");
}

#[test]
fn two_panes_on_one_conversation_share_its_messages_and_load_it_once() {
    let mut d = Demo::new(200, 50);
    d.open("backend");
    let target = d.app.open_target().unwrap().clone();
    d.app.open_beside(target.clone());
    assert!(d.app.take_commands().is_empty(), "the second pane loads nothing");
    let panes = d.app.open_panes();
    assert_eq!(panes.len(), 2);
    assert!(!panes[0].messages().is_empty() && panes[0].messages() == panes[1].messages());
    assert_eq!(d.app.focused_pane().map(|p| p.handle()), Some(panes[1].handle()));
    assert!(d.app.pane_showing(&target).is_some());
    // Written in one, sent: the message is in both, and the draft is gone from both.
    d.keys("i");
    d.type_text("both");
    d.keys("enter esc");
    for p in d.app.open_panes() {
        assert_eq!(p.messages().last().unwrap().text.as_str(), "both");
        assert!(p.composer_text().is_empty());
    }
}

#[test]
fn a_draft_is_kept_when_its_conversation_closes_and_quitting_asks_about_it() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.keys("i");
    d.type_text("half written");
    d.keys("esc ctrl+w");
    assert!(d.app.open_target().is_none());
    d.open("incidents");
    assert_eq!(conversation(&d).composer_text(), "", "another conversation, another draft");
    d.open("backend");
    assert_eq!(conversation(&d).composer_text(), "half written", "back where it was left");
    d.open("incidents");
    d.keys("ctrl+q");
    assert!(!d.app.quit);
    assert_eq!(d.app.overlay(), Some(Overlay::Dialog(Question::Quit)), "a draft of a closed pane is not sent");
}

#[test]
fn the_focus_never_names_a_closed_pane() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    d.keys("k enter");
    let thread = d.app.focused_pane().map(PaneRef::handle).expect("the thread panel has the keyboard");
    d.keys("ctrl+w");
    assert!(d.app.pane_for(thread).is_none(), "closed");
    assert_eq!(d.focused_kind(), Some(PaneKind::Conversation), "back to the pane that opened it");
    let main = d.app.focused_pane().unwrap().handle();
    d.keys("ctrl+w");
    assert!(d.app.pane_for(main).is_none());
    assert_eq!(d.app.focus(), Focus::List, "nothing open: the list");
    // The list hidden: the keyboard is in the empty work area, which is no pane.
    d.keys("space e");
    assert_eq!(d.app.focus(), Focus::Work);
    assert!(d.app.focused_pane().is_none());
    d.keys("esc");
    assert_eq!(d.app.focus(), Focus::List, "Esc brings the list back");
}
