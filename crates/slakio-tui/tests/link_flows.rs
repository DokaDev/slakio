//! Links opened in the browser: `o` opens the selected message's link when one opens; with
//! several, letters are drawn on each (`a`, `s`, `d` …) and typing them opens that one, or a
//! list shows when one is off screen; a click on a link opens it; `:open <address>` the address
//! typed. Only a web address (`http`, `https`) is handed to the binary (`Effect::OpenLink`; no
//! test starts a browser), the host it goes to said; a link whose text names another host is
//! asked about first, the real host shown; `javascript:`, `data:`, `file:`, `slack:` and the like
//! are refused with a reason. Nothing goes to Slack, and a read-only workspace opens links too.
//! At 120x40 and 80x24, in English and Korean.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use scripted::Scripted;
use slakio_core::backend::Event;
use slakio_core::i18n::{I18n, Label, Lang};
use slakio_core::model::{Message, Ts};
use slakio_core::sanitize::Remote;
use slakio_tui::app::Effect;

/// Read-only, booted, `deploys` open at `w`×`h` in `lang`, its page the messages `texts`
/// (oldest first), the newest selected.
fn shown(texts: &[&str], w: u16, h: u16, lang: Lang) -> Scripted {
    let mut s = Scripted::booted(true, None);
    s.d.app.i18n = I18n::new(lang);
    s.d.app.resize(w, h);
    s.d.list_cursor_on("deploys");
    s.keys("enter");
    let mut e = s.b.reply(0);
    let Event::History(page) = &mut e.event else { panic!("a page") };
    let author = page.messages[0].user.clone();
    page.messages = (texts.iter().enumerate())
        .map(|(i, t)| Message {
            ts: Ts(1_767_603_600_000_000 + i as u64 * 600_000_000),
            user: author.clone(),
            text: Remote::new((*t).to_string()),
            thread: None,
            reactions: Vec::new(),
            edited: false,
        })
        .collect();
    page.complete = true;
    s.b.push(e);
    s.pump();
    s.keys("G");
    // The binary draws after every key; `o` places its letters where the links were drawn.
    s.d.screen();
    s
}

/// `keys`, the screen drawn after them (as the binary does).
fn press(s: &mut Scripted, keys: &str) {
    s.keys(keys);
    s.d.screen();
}

/// The addresses handed to the binary to open since the last call.
fn opened(s: &mut Scripted) -> Vec<String> {
    let effects = s.d.app.take_effects();
    effects.into_iter().filter_map(|e| if let Effect::OpenLink(u) = e { Some(u) } else { None }).collect()
}

fn command(s: &mut Scripted, line: &str) {
    s.keys(":");
    s.d.type_text(line);
    s.keys("enter");
}

#[test]
fn open_with_a_number_is_an_address_typed_and_refused() {
    let mut s = shown(&["see <https://example.com/a> or <https://example.com/b>"], 80, 24, Lang::En);
    for n in ["0", "1", "2"] {
        command(&mut s, &format!("open {n}"));
        assert!(opened(&mut s).is_empty(), "{n}");
        let refused = s.d.app.i18n.label(Label::OpenRefused).to_string();
        assert!(s.d.status_line().contains(&refused), "{n}: {}", s.d.status_line());
    }
    // `:open` alone is `o`.
    command(&mut s, "open");
    assert!(s.d.app.picker().is_some());
}

const SIZES: [(Lang, u16, u16); 3] = [(Lang::En, 120, 40), (Lang::En, 80, 24), (Lang::Ko, 80, 24)];

#[test]
fn o_opens_the_one_link_at_once_and_picks_among_several_saying_the_host() {
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let i18n = I18n::new(lang);
        let text = "see <https://docs.example.com/runbooks/deploy|the runbook> or <https://status.example.com/x?y=1>";
        let mut s = shown(&[text], w, h, lang);
        let asked = s.b.log.len() + s.b.held.len();
        press(&mut s, "o");
        assert!(opened(&mut s).is_empty(), "{at}: two links, one is picked");
        press(&mut s, "a");
        assert_eq!(opened(&mut s), ["https://docs.example.com/runbooks/deploy"], "{at}");
        let said = i18n.msg(&slakio_core::i18n::Msg::OpenOpening { host: "docs.example.com".into() }).to_string();
        assert!(s.d.status_line().contains(&said), "{at}: {}", s.d.status_line());
        press(&mut s, "o");
        press(&mut s, "s");
        assert_eq!(opened(&mut s), ["https://status.example.com/x?y=1"], "{at}");
        // A message with one link opens it at once.
        let mut s = shown(&["see <https://docs.example.com/one>"], w, h, lang);
        s.keys("o");
        assert_eq!(opened(&mut s), ["https://docs.example.com/one"], "{at}");
        assert!(s.d.app.picker().is_none(), "{at}");
        // A local action: nothing was asked of Slack, read-only as the workspace is.
        assert_eq!(s.b.log.len() + s.b.held.len(), asked, "{at}");
        assert!(s.d.app.question().is_none(), "{at}");
    }
}

#[test]
fn a_link_whose_text_names_another_host_is_asked_about_first_showing_the_real_host() {
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let mut s = shown(&["<https://login.evil.test/x|https://docs.example.com/login>"], w, h, lang);
        s.keys("o");
        assert!(opened(&mut s).is_empty(), "{at}: asked first");
        let screen = s.d.screen();
        assert!(screen.contains("login.evil.test") && screen.contains("docs.example.com"), "{at}: {screen}");
        // `Enter` alone keeps the safe answer: nothing opens.
        s.keys("enter");
        assert!(opened(&mut s).is_empty() && s.d.app.question().is_none(), "{at}");
        s.keys("o");
        s.keys("n");
        assert!(opened(&mut s).is_empty(), "{at}");
        s.keys("o");
        s.keys("y");
        assert_eq!(opened(&mut s), ["https://login.evil.test/x"], "{at}");
        // After the yes, the status line names where it goes, as when nothing is asked.
        let said =
            s.d.app.i18n.msg(&slakio_core::i18n::Msg::OpenOpening { host: "login.evil.test".into() }).to_string();
        assert!(s.d.status_line().contains(&said), "{at}: {}", s.d.status_line());
        // Text that names the same host, or no host at all, opens at once.
        let mut s =
            shown(&["<https://docs.example.com/a|docs.example.com> <https://docs.example.com/b|here>"], w, h, lang);
        press(&mut s, "o");
        press(&mut s, "a");
        assert_eq!(opened(&mut s), ["https://docs.example.com/a"], "{at}");
        press(&mut s, "o");
        press(&mut s, "s");
        assert_eq!(opened(&mut s), ["https://docs.example.com/b"], "{at}");
        assert!(s.d.app.question().is_none(), "{at}");
    }
}

#[test]
fn only_web_addresses_open_the_others_are_refused_with_a_reason() {
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let refused = I18n::new(lang).label(Label::OpenRefused).to_string();
        for link in [
            "<javascript:alert(1)|https://docs.example.com>",
            "<data:text/html,hi|click>",
            "<file:///etc/passwd>",
            "<slack://channel?team=T1&id=C1|the channel>",
            "<mailto:a@example.com>",
            "<https://user:pw@example.com/|example.com>",
        ] {
            let mut s = shown(&[link], w, h, lang);
            s.keys("o");
            assert!(opened(&mut s).is_empty(), "{at} {link}");
            assert!(s.d.app.question().is_none(), "{at} {link}");
            assert!(s.d.status_line().contains(&refused), "{at} {link}: {}", s.d.status_line());
        }
        let mut s = shown(&["no link in here"], w, h, lang);
        s.keys("o");
        assert!(opened(&mut s).is_empty(), "{at}");
        // Typed: a web address opens, anything else is refused.
        command(&mut s, "open https://example.com/typed?q=1");
        assert_eq!(opened(&mut s), ["https://example.com/typed?q=1"], "{at}");
        command(&mut s, "open javascript:alert(1)");
        assert!(opened(&mut s).is_empty(), "{at}");
        assert!(s.d.status_line().contains(&refused), "{at}: {}", s.d.status_line());
    }
}

/// The text of the confirmation dialog alone: its rows between the box's corners, so the message
/// list behind it is left out.
fn dialog_rows(screen: &str, title: &str) -> String {
    let lines: Vec<&str> = screen.lines().collect();
    let top = lines.iter().position(|l| l.contains(&format!("╭ {title}"))).expect("the dialog");
    let left = lines[top].chars().position(|c| c == '╭').unwrap();
    let bottom = lines.iter().skip(top).position(|l| l.chars().nth(left) == Some('╰')).expect("its bottom") + top;
    // From the box's left border to its right one (a wide character moves the corners).
    let rows = lines[top + 1..bottom].iter().map(|l| {
        let inside: String = l.chars().skip(left + 1).collect();
        inside.trim_end().trim_end_matches('│').trim_end().trim_end_matches('│').trim().to_string()
    });
    rows.collect::<Vec<_>>().join("\n")
}

/// A host of about 200 characters that starts like a trusted one and ends with another's.
fn long_host() -> String {
    format!("docs.google.com.{}.{}.{}.evil.test", "x".repeat(60), "y".repeat(60), "z".repeat(52))
}

// 50 columns is the narrowest screen slakio draws (below it says the terminal is too small).
#[test]
fn a_long_host_is_shown_first_and_cut_from_its_start_so_its_end_stays() {
    let host = long_host();
    assert!(host.len() > 190 && host.len() <= 253, "{}", host.len());
    for (lang, w, h) in [(Lang::En, 50, 24), (Lang::Ko, 50, 24), (Lang::En, 80, 24), (Lang::En, 120, 40)] {
        let at = format!("{lang:?} {w}x{h}");
        let text = format!("<https://{host}/x|https://docs.google.com/x> <https://{host}/y>");
        let mut s = shown(&[text.as_str()], w, h, lang);
        press(&mut s, "o");
        // Off screen or not, its letter (first) or its row in the list picks it.
        let listed = s.d.app.picker().is_some_and(|p| p.list.is_some());
        press(&mut s, if listed { "enter" } else { "a" });
        let screen = s.d.screen();
        let dialog = dialog_rows(&screen, &s.d.app.i18n.label(Label::DialogOpenTitle));
        assert!(dialog.contains("zz.evil.test"), "{at}: the real host's end: {dialog}");
        assert!(!dialog.contains("docs.google.com.x"), "{at}: the host's start: {dialog}");
        // The real host comes before what the link's text says, which is there.
        let (host_at, said_at) = (dialog.find("evil.test").unwrap(), dialog.find("docs.google.com/x"));
        assert!(said_at.is_some_and(|said| host_at < said), "{at}: {dialog}");
        s.keys("n");
        // Without text of its own, the status line names the host's end.
        press(&mut s, "o");
        press(&mut s, "s");
        assert_eq!(opened(&mut s), [format!("https://{host}/y")], "{at}");
        let status = s.d.status_line();
        if w >= 80 {
            assert!(status.contains("…") && status.contains("zz.evil.test"), "{at}: {status}");
        }
    }
    // Longer than the DNS allows: refused.
    let mut s = shown(&[&format!("<https://{}.{host}/>", "w".repeat(60))], 80, 24, Lang::En);
    s.keys("o");
    assert!(opened(&mut s).is_empty());
}

/// Three web links among a `javascript:` one and a `file:` one.
const MIXED: &str = "a <javascript:alert(1)|click> b <https://one.example.org/1|first> c <file:///etc/x|f> \
                     d <https://two.example.net/2|second> e <https://three.example.com/3>";

/// The screen drawn, then the time a click needs after the links moved gone by.
fn settle(s: &mut Scripted) {
    s.d.screen();
    s.d.now += std::time::Duration::from_millis(500);
}

/// The cell at `x`, `y` is drawn in the hint style.
fn is_hint(s: &Scripted, x: u16, y: u16) -> bool {
    let cell = &s.d.buffer()[(x, y)];
    cell.style().bg == s.d.app.theme.hint().bg && cell.modifier.contains(ratatui::style::Modifier::BOLD)
}

/// The cell where `word` starts on the screen of `s` (column, row).
fn cell_of(s: &Scripted, word: &str) -> (u16, u16) {
    let screen = s.d.screen();
    for (y, line) in screen.lines().enumerate() {
        if let Some(b) = line.find(word) {
            return (unicode_width::UnicodeWidthStr::width(&line[..b]) as u16, y as u16);
        }
    }
    panic!("{word} is not on the screen: {screen}")
}

#[test]
fn several_links_get_letters_on_them_and_the_second_letter_opens_the_second_web_link() {
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let mut s = shown(&[MIXED], w, h, lang);
        press(&mut s, "o");
        assert!(opened(&mut s).is_empty(), "{at}");
        let p = s.d.app.picker().expect("picking");
        assert!(p.list.is_none(), "{at}: all on screen, letters on them");
        let hints: Vec<(&str, &str)> = p.picks.iter().map(|k| (k.hint.as_str(), k.host.as_str())).collect();
        assert_eq!(hints, [("a", "one.example.org"), ("s", "two.example.net"), ("d", "three.example.com")], "{at}");
        // The letters are drawn over the links' first cells, in the hint style; the refused links
        // get none.
        let screen = s.d.screen();
        assert!(
            screen.contains("airst") && screen.contains("second") && screen.contains("dttps://three"),
            "{at}: {screen}"
        );
        assert!(screen.contains("a click b") && screen.contains("c f d"), "{at}: {screen}");
        let buf = s.d.buffer();
        let hint = s.d.app.theme.hint();
        let (x, y) = cell_of(&s, "airst");
        assert_eq!(buf[(x, y)].style().bg, hint.bg, "{at}");
        assert!(buf[(x, y)].modifier.contains(ratatui::style::Modifier::BOLD), "{at}");
        let (sx, sy) = cell_of(&s, "second");
        assert!(is_hint(&s, sx, sy), "{at}: s on second");
        // Refused links are plain text: not underlined as web links are.
        let (x, y) = cell_of(&s, "click");
        assert!(!buf[(x, y)].modifier.contains(ratatui::style::Modifier::UNDERLINED), "{at}");
        let (x, y) = cell_of(&s, "irst");
        assert!(buf[(x, y)].modifier.contains(ratatui::style::Modifier::UNDERLINED), "{at}");
        let label = |l: Label| s.d.app.i18n.label(l).to_string();
        let keys = format!(
            "a/s/d {} · o {} · Esc {}",
            label(Label::HintOpen),
            label(Label::HintList),
            label(Label::HintCancel)
        );
        assert!(s.d.status_line().contains(&keys), "{at}: {}", s.d.status_line());
        press(&mut s, "s");
        assert_eq!(opened(&mut s), ["https://two.example.net/2"], "{at}");
        assert!(s.d.app.picker().is_none(), "{at}");
        assert!(!is_hint(&s, sx, sy) && s.d.screen().contains("first"), "{at}: the letters are gone");
    }
}

#[test]
fn esc_or_a_key_that_picks_nothing_stops_picking_and_opens_nothing() {
    let mut s = shown(&[MIXED], 80, 24, Lang::En);
    press(&mut s, "o");
    press(&mut s, "esc");
    assert!(s.d.app.picker().is_none() && opened(&mut s).is_empty());
    press(&mut s, "s");
    assert!(opened(&mut s).is_empty(), "no longer picking: `s` splits the pane");
    press(&mut s, "ctrl+w");
    press(&mut s, "o");
    press(&mut s, "z");
    assert!(s.d.app.picker().is_none() && opened(&mut s).is_empty());
    // Hangul typed with a Korean input source picks by the key under it (U+3134 is `s`).
    press(&mut s, "o");
    s.d.type_text("\u{3134}");
    assert_eq!(opened(&mut s), ["https://two.example.net/2"]);
    // A message whose links all are refused says why, and picks nothing.
    let mut s = shown(&["<javascript:x|a> <file:///y> <mailto:a@example.com>"], 80, 24, Lang::En);
    s.keys("o");
    assert!(s.d.app.picker().is_none() && opened(&mut s).is_empty());
    assert!(s.d.status_line().contains(&s.d.app.i18n.label(Label::OpenRefused).to_string()));
}

#[test]
fn links_off_the_screen_are_picked_from_a_list_host_first() {
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let filler = "line\n".repeat(45);
        let text = format!("<https://top.example.com/t|the top>\n{filler}<https://end.example.org/e|the end>");
        let mut s = shown(&[text.as_str()], w, h, lang);
        press(&mut s, "o");
        assert!(s.d.app.picker().is_some_and(|p| p.list.is_some()), "{at}: a link is off screen");
        let screen = s.d.screen();
        let title = s.d.app.i18n.label(Label::OpenListTitle).to_string();
        assert!(screen.contains(&title), "{at}: {screen}");
        let row = screen.lines().find(|l| l.contains("top.example.com")).expect("its row");
        assert!(row.find("top.example.com") < row.find("the top"), "{at}: host first: {row}");
        press(&mut s, "j");
        press(&mut s, "enter");
        assert_eq!(opened(&mut s), ["https://end.example.org/e"], "{at}");
        assert!(!s.d.screen().contains(&title), "{at}");
        // Its letters pick in the list too; Esc closes it.
        press(&mut s, "o");
        press(&mut s, "a");
        assert_eq!(opened(&mut s), ["https://top.example.com/t"], "{at}");
        press(&mut s, "o");
        press(&mut s, "esc");
        assert!(s.d.app.picker().is_none() && opened(&mut s).is_empty(), "{at}");
    }
    // Letters on screen, `o` again lists them.
    let mut s = shown(&[MIXED], 80, 24, Lang::En);
    press(&mut s, "o");
    press(&mut s, "o");
    assert!(s.d.app.picker().is_some_and(|p| p.list.is_some()));
    press(&mut s, "k");
    press(&mut s, "j");
    press(&mut s, "enter");
    assert_eq!(opened(&mut s), ["https://two.example.net/2"]);
}

#[test]
fn a_click_on_a_link_opens_it_and_on_a_refused_one_says_why() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let mut s = shown(&[MIXED, "an older one"], w, h, lang);
        s.keys("k");
        settle(&mut s);
        let (x, y) = cell_of(&s, "second");
        s.d.mouse(MouseEventKind::Down(MouseButton::Left), x + 2, y);
        assert_eq!(opened(&mut s), ["https://two.example.net/2"], "{at}");
        settle(&mut s);
        let (x, y) = cell_of(&s, "click");
        s.d.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
        assert!(opened(&mut s).is_empty(), "{at}");
        let refused = s.d.app.i18n.label(Label::OpenRefused).to_string();
        assert!(s.d.status_line().contains(&refused), "{at}: {}", s.d.status_line());
        // A click beside a link selects the message and opens nothing.
        s.d.screen();
        let (x, y) = cell_of(&s, "a click");
        s.d.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
        assert!(opened(&mut s).is_empty(), "{at}");
    }
}

#[test]
fn the_thread_pane_picks_its_replies_links_too() {
    use slakio_core::backend::Command;
    use slakio_core::model::Target;
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let mut s = shown(&["the root"], w, h, lang);
        s.keys("enter");
        let i =
            s.b.held.iter().position(|r| matches!(&r.command, Command::History { target: Target::Thread { .. }, .. }));
        let mut e = s.b.reply(i.expect("the thread asked for"));
        let Event::History(page) = &mut e.event else { panic!("a page") };
        let user = s.d.pane(slakio_tui::app::PaneKind::Conversation).unwrap().messages()[0].user.clone();
        let message = |ts: u64, text: &str| Message {
            ts: Ts(ts),
            user: user.clone(),
            text: Remote::new(text.to_string()),
            thread: None,
            reactions: Vec::new(),
            edited: false,
        };
        page.messages = vec![message(1_767_603_600_000_000, "the root"), message(1_767_603_700_000_000, MIXED)];
        page.complete = true;
        s.b.push(e);
        s.pump();
        press(&mut s, "G");
        press(&mut s, "o");
        assert!(s.d.app.picker().is_some_and(|p| p.list.is_none()), "{at}");
        assert!(s.d.screen().contains("dttps://"), "{at}: {}", s.d.screen());
        press(&mut s, "d");
        assert_eq!(opened(&mut s), ["https://three.example.com/3"], "{at}");
    }
}

#[test]
fn the_keys_row_of_o_is_whole_in_the_help_and_the_palette_at_80_columns() {
    for lang in [Lang::En, Lang::Ko] {
        let mut s = shown(&["no link"], 80, 24, lang);
        let label = s.d.app.i18n.label(Label::ActionPaneOpenLink).to_string();
        press(&mut s, "?");
        press(&mut s, "/");
        s.d.type_text(":open");
        let screen = s.d.screen();
        assert!(screen.lines().any(|l| l.contains(&label)), "{lang:?} help: {screen}");
        press(&mut s, "esc");
        press(&mut s, "esc");
        s.keys(":");
        s.d.type_text("open");
        let screen = s.d.screen();
        assert!(screen.lines().any(|l| l.contains(&label)), "{lang:?} palette: {screen}");
    }
}

#[test]
fn past_twelve_links_the_first_get_one_letter_the_rest_two_and_both_pick_it() {
    let text: String = (0..15).map(|i| format!("<https://h{i}.example.com/|link{i}> ")).collect();
    let mut s = shown(&[text.as_str()], 120, 40, Lang::En);
    press(&mut s, "o");
    let hints: Vec<String> = s.d.app.picker().unwrap().picks.iter().map(|k| k.hint.clone()).collect();
    assert_eq!(hints[..11].join(" "), "a s d f g h l e r w u");
    assert_eq!(hints[11..], ["ia", "is", "id", "if"]);
    press(&mut s, "i");
    assert!(opened(&mut s).is_empty() && s.d.app.picker().is_some(), "one letter is not a pick yet");
    press(&mut s, "s");
    assert_eq!(opened(&mut s), ["https://h12.example.com/"]);
}

/// `n` web links, `<https://hN.example.com/|lN>`, in one message.
fn many(n: usize) -> String {
    (0..n).map(|i| format!("<https://h{i}.example.com/|l{i}> ")).collect()
}

#[test]
fn moving_or_letting_go_of_the_mouse_keeps_picking_and_a_press_stops_it_and_redraws() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let mut s = shown(&[MIXED], 80, 24, Lang::En);
    press(&mut s, "o");
    let (x, y) = cell_of(&s, "airst");
    for kind in [MouseEventKind::Moved, MouseEventKind::Up(MouseButton::Left), MouseEventKind::Drag(MouseButton::Left)]
    {
        assert!(!s.d.mouse(kind, 3, 3), "{kind:?}: nothing to draw");
        assert!(s.d.app.picker().is_some_and(|p| p.list.is_none()), "{kind:?}: still picking");
    }
    assert!(is_hint(&s, x, y), "the letters still show");
    press(&mut s, "s");
    assert_eq!(opened(&mut s), ["https://two.example.net/2"], "a letter after a move still picks");
    // The list too.
    press(&mut s, "o");
    press(&mut s, "o");
    assert!(!s.d.mouse(MouseEventKind::Moved, 3, 3));
    assert!(s.d.app.picker().is_some_and(|p| p.list.is_some()), "the list stays");
    press(&mut s, "esc");
    // A press stops the letters, and the screen is drawn again without them.
    press(&mut s, "o");
    assert!(s.d.mouse(MouseEventKind::Down(MouseButton::Right), 3, 3), "drawn again");
    assert!(s.d.app.picker().is_none());
    assert!(!is_hint(&s, x, y), "the letters are gone");
    // The next letter is a key of the app again, not a pick.
    press(&mut s, "s");
    assert!(opened(&mut s).is_empty());
}

#[test]
fn j_and_k_move_the_list_and_no_link_is_labelled_with_them() {
    let mut s = shown(&[many(6).as_str()], 80, 24, Lang::En);
    press(&mut s, "o");
    let hints: Vec<String> = s.d.app.picker().unwrap().picks.iter().map(|k| k.hint.clone()).collect();
    assert_eq!(hints.join(" "), "a s d f g h");
    // With letters on the links, `j` shows the list: it opens nothing.
    press(&mut s, "j");
    assert!(opened(&mut s).is_empty());
    assert_eq!(s.d.app.picker().and_then(|p| p.list), Some(0));
    press(&mut s, "j");
    press(&mut s, "j");
    press(&mut s, "k");
    assert_eq!(s.d.app.picker().and_then(|p| p.list), Some(1));
    // The fifth link's label picks it in the list too.
    press(&mut s, "g");
    assert_eq!(opened(&mut s), ["https://h4.example.com/"]);
    press(&mut s, "o");
    press(&mut s, "k");
    press(&mut s, "enter");
    assert_eq!(opened(&mut s), ["https://h0.example.com/"]);
}

#[test]
fn the_list_scrolls_with_its_cursor_and_opens_only_a_row_shown() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let mut s = shown(&[many(30).as_str()], 80, 24, Lang::En);
    press(&mut s, "o");
    press(&mut s, "o");
    for _ in 0..25 {
        press(&mut s, "down");
    }
    let screen = s.d.screen();
    let title = s.d.app.i18n.label(Label::OpenListTitle).to_string();
    assert!(screen.contains("h25.example.com") && screen.contains(&format!("{title} 26/30")), "{screen}");
    assert!(!screen.contains("h0.example.com"), "scrolled: {screen}");
    // The last row shown is clicked: that one opens, counted from the first row shown.
    let (x, y) = cell_of(&s, "h25.example.com");
    s.d.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert_eq!(opened(&mut s), ["https://h25.example.com/"]);
    // The wheel moves the cursor; Enter opens the row under it, which is shown.
    press(&mut s, "o");
    press(&mut s, "o");
    for _ in 0..40 {
        s.d.mouse(MouseEventKind::ScrollDown, x, y);
    }
    assert!(s.d.screen().contains("h29.example.com"));
    press(&mut s, "enter");
    assert_eq!(opened(&mut s), ["https://h29.example.com/"]);
}

#[test]
fn past_the_most_labels_the_list_still_has_every_link() {
    let mut s = shown(&[many(150).as_str()], 80, 24, Lang::En);
    press(&mut s, "o");
    let p = s.d.app.picker().expect("picking");
    assert_eq!(p.picks.len(), 150);
    assert!(p.list.is_some(), "some links have no label: the list");
    assert!(p.picks[149].hint.is_empty() && !p.picks[143].hint.is_empty());
    for _ in 0..149 {
        s.keys("j");
    }
    press(&mut s, "enter");
    assert_eq!(opened(&mut s), ["https://h149.example.com/"]);
}

#[test]
fn a_double_click_on_a_link_opens_it_once() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let mut s = shown(&[MIXED], 80, 24, Lang::En);
    settle(&mut s);
    let (x, y) = cell_of(&s, "second");
    s.d.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    s.d.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert_eq!(opened(&mut s), ["https://two.example.net/2"]);
}

#[test]
fn a_click_just_after_the_links_moved_only_selects() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let mut s = shown(&["an older one", MIXED], 80, 24, Lang::En);
    s.keys("k");
    // Drawn just now (the view moved): the link under the pointer may not be the one aimed at.
    s.d.screen();
    let (x, y) = cell_of(&s, "second");
    s.d.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert!(opened(&mut s).is_empty());
    let selected = s.d.pane(slakio_tui::app::PaneKind::Conversation).unwrap().selected();
    assert_eq!(selected, Some(1), "the message is selected");
    s.d.now += std::time::Duration::from_millis(500);
    s.d.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert_eq!(opened(&mut s), ["https://two.example.net/2"]);
}

/// The message at `ts` of `deploys` in `s` now reads `text` (edited live).
fn edit(s: &mut Scripted, ts: Ts, text: &str) {
    use slakio_core::backend::{Envelope, Generation, Live};
    let c = s.d.app.model.conversations().iter().find(|c| c.name == "deploys").unwrap().clone();
    let user = s.d.pane(slakio_tui::app::PaneKind::Conversation).unwrap().messages()[0].user.clone();
    let message =
        Message { ts, user, text: Remote::new(text.to_string()), thread: None, reactions: Vec::new(), edited: true };
    let live = Live::Message {
        workspace: c.workspace,
        conversation: c.id,
        thread: None,
        broadcast: false,
        changed: true,
        mention: false,
        message,
        users: Vec::new(),
    };
    s.b.push(Envelope { generation: Generation::PUSHED, event: Event::Live(Box::new(live)) });
    s.pump();
}

/// The time of the newest message of `shown`, `n` messages in.
fn ts_of(n: usize) -> Ts {
    Ts(1_767_603_600_000_000 + (n as u64 - 1) * 600_000_000)
}

#[test]
fn a_message_edited_while_picking_or_before_a_click_opens_nothing() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let mut s = shown(&[MIXED], 80, 24, Lang::En);
    press(&mut s, "o");
    edit(&mut s, ts_of(1), "now <https://evil.example.org/x|first> and <https://evil.example.org/y|second>");
    // Not drawn again yet: the letter typed was for the message as it read.
    s.keys("s");
    assert!(opened(&mut s).is_empty());
    let changed = s.d.app.i18n.label(Label::OpenChanged).to_string();
    assert!(s.d.status_line().contains(&changed), "{}", s.d.status_line());
    // A click on a link as drawn, the message edited since (not drawn again): nothing opens.
    let mut s = shown(&[MIXED], 80, 24, Lang::En);
    settle(&mut s);
    let (x, y) = cell_of(&s, "second");
    // Laid out as before: only where `second` goes changed.
    edit(&mut s, ts_of(1), &MIXED.replace("two.example.net", "evil.example.org"));
    s.d.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    assert!(opened(&mut s).is_empty());
    assert!(s.d.status_line().contains(&changed), "{}", s.d.status_line());
}

#[test]
fn a_list_row_a_wrapped_links_second_row_and_its_host_open_on_a_click() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        // A list row.
        let mut s = shown(&[MIXED], w, h, lang);
        press(&mut s, "o");
        press(&mut s, "o");
        let (x, y) = cell_of(&s, "two.example.net");
        s.d.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
        assert_eq!(opened(&mut s), ["https://two.example.net/2"], "{at}");
        // A click outside the list closes it.
        press(&mut s, "o");
        press(&mut s, "o");
        s.d.mouse(MouseEventKind::Down(MouseButton::Left), 0, 0);
        assert!(s.d.app.picker().is_none() && opened(&mut s).is_empty(), "{at}");
        // The second row of a link that wraps.
        let words = "wraps ".repeat(30);
        let text = format!("x <https://long.example.com/w|{} tail-of-the-link>", words.trim());
        let mut s = shown(&[text.as_str()], w, h, lang);
        settle(&mut s);
        let (x, y) = cell_of(&s, "tail-of-the-link");
        let (_, first) = cell_of(&s, "wraps");
        assert!(y > first, "{at}: it wraps");
        s.d.mouse(MouseEventKind::Down(MouseButton::Left), x + 3, y);
        assert_eq!(opened(&mut s), ["https://long.example.com/w"], "{at}");
        // The `→ host` after a link whose text names another host: asked about first.
        let mut s = shown(&["<https://login.evil.test/x|https://docs.example.com/login>"], w, h, lang);
        settle(&mut s);
        let (x, y) = cell_of(&s, "login.evil.test");
        s.d.mouse(MouseEventKind::Down(MouseButton::Left), x + 2, y);
        assert!(opened(&mut s).is_empty() && s.d.app.question().is_some(), "{at}: asked first");
    }
}

#[test]
fn compact_messages_open_their_links_on_a_click_too() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let mut s = shown(&["an older one", MIXED], w, h, lang);
        command(&mut s, "density compact");
        let _ = s.d.app.take_effects();
        settle(&mut s);
        let (x, y) = cell_of(&s, "second");
        s.d.mouse(MouseEventKind::Down(MouseButton::Left), x + 1, y);
        assert_eq!(opened(&mut s), ["https://two.example.net/2"], "{at}");
        settle(&mut s);
        let (x, y) = cell_of(&s, "first");
        s.d.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
        assert_eq!(opened(&mut s), ["https://one.example.org/1"], "{at}");
    }
}

#[test]
fn letters_draw_over_the_links_so_nothing_moves_and_case_does_not_matter() {
    let mut s = shown(&[MIXED], 80, 24, Lang::En);
    let before = cell_of(&s, "econd");
    let rows = s.d.screen().lines().count();
    press(&mut s, "o");
    assert_eq!(cell_of(&s, "econd"), before, "the link did not move");
    assert_eq!(s.d.screen().lines().count(), rows);
    // Shift (or Caps Lock) types the same letter.
    press(&mut s, "S");
    assert_eq!(opened(&mut s), ["https://two.example.net/2"]);
}

#[test]
fn after_a_first_letter_only_its_labels_show_and_backspace_takes_it_back() {
    let mut s = shown(&[many(15).as_str()], 120, 40, Lang::En);
    press(&mut s, "o");
    // `l0`: its `l` is under the letter `a`.
    let (x0, y0) = cell_of(&s, "a0 s1");
    assert!(is_hint(&s, x0, y0));
    press(&mut s, "i");
    assert!(!is_hint(&s, x0, y0), "`a` is not started by `i`: hidden");
    // `ia` labels the twelfth link: `a` is left to type, drawn on it.
    let screen = s.d.screen();
    assert!(screen.contains("a11") && screen.contains("s12"), "{screen}");
    let back = s.d.app.i18n.label(Label::HintBack).to_string();
    assert!(s.d.status_line().contains(&format!("Backspace {back}")), "{}", s.d.status_line());
    press(&mut s, "backspace");
    assert!(s.d.app.picker().is_some_and(|p| p.typed.is_empty()));
    assert!(is_hint(&s, x0, y0), "back: every label again");
    press(&mut s, "backspace");
    assert!(s.d.app.picker().is_some(), "nothing to take back: still picking");
    press(&mut s, "w");
    assert_eq!(opened(&mut s), ["https://h9.example.com/"]);
}

#[test]
fn the_picking_keys_say_what_they_do_at_80_columns_and_in_the_help() {
    for lang in [Lang::En, Lang::Ko] {
        let mut s = shown(&[MIXED], 80, 24, lang);
        let label = |l: Label| s.d.app.i18n.label(l).to_string();
        let keys = format!(
            "a/s/d {} · o {} · Esc {}",
            label(Label::HintOpen),
            label(Label::HintList),
            label(Label::HintCancel)
        );
        press(&mut s, "o");
        assert!(s.d.status_line().contains(&keys), "{lang:?}: {}", s.d.status_line());
        press(&mut s, "esc");
        press(&mut s, "?");
        press(&mut s, "/");
        let back = s.d.app.i18n.label(Label::ActionLinksBack).to_string();
        s.d.type_text(&back);
        let screen = s.d.screen();
        assert!(screen.contains(&back) && screen.contains("Backspace"), "{lang:?}: {screen}");
    }
}

#[test]
fn no_link_says_so_the_same_way_with_or_without_a_selection() {
    use slakio_tui::app::status::Level;
    let mut s = shown(&["no link here"], 80, 24, Lang::En);
    s.keys("o");
    let level = s.d.app.status.notice(s.d.now).map(|n| n.level);
    assert_eq!(level, Some(Level::Info));
    s.keys("esc");
    s.keys("o");
    let level = s.d.app.status.notice(s.d.now).map(|n| n.level);
    assert_eq!(level, Some(Level::Info));
}

#[test]
fn an_open_after_a_refusal_says_it_is_opening_at_once() {
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let mut s = shown(&["see <https://docs.example.com/a>"], w, h, lang);
        command(&mut s, "open javascript:alert(1)");
        let refused = s.d.app.i18n.label(Label::OpenRefused).to_string();
        assert!(s.d.status_line().contains(&refused), "{at}: {}", s.d.status_line());
        // A second later the link opens: the refusal is no longer the news.
        s.d.now += std::time::Duration::from_secs(1);
        press(&mut s, "o");
        assert_eq!(opened(&mut s), ["https://docs.example.com/a"], "{at}");
        let said = s.d.app.i18n.msg(&slakio_core::i18n::Msg::OpenOpening { host: "docs.example.com".into() });
        let line = s.d.status_line();
        assert!(line.contains(said.as_str()) && !line.contains(&refused), "{at}: {line}");
    }
}

#[test]
fn a_long_host_is_said_whole_where_the_status_line_has_the_room() {
    let host = "build-artifacts.delta.h0.example.com";
    let mut s = shown(&[&format!("see <https://{host}/x>")], 120, 40, Lang::En);
    press(&mut s, "o");
    assert_eq!(opened(&mut s).len(), 1);
    assert!(s.d.status_line().contains(&format!("Opening {host}")), "{}", s.d.status_line());
    // At 80 columns its end is said, so the words fit.
    let mut s = shown(&[&format!("see <https://{host}/x>")], 80, 24, Lang::En);
    press(&mut s, "o");
    assert_eq!(opened(&mut s).len(), 1);
    assert!(s.d.status_line().contains("Opening …lta.h0.example.com"), "{}", s.d.status_line());
}

#[test]
fn the_help_rows_of_the_link_list_are_whole_at_80_columns() {
    // (The Korean rows were whole already.)
    let mut s = shown(&["see <https://docs.example.com/a>"], 80, 24, Lang::En);
    s.keys("?");
    s.keys("/");
    s.d.type_text("link");
    let screen = s.d.screen();
    for l in [Label::ActionLinksDown, Label::ActionLinksUp] {
        let label = s.d.app.i18n.label(l).to_string();
        assert!(screen.contains(&label), "{label} is whole:\n{screen}");
    }
}
