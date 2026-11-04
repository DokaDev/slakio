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
        // The letters are drawn on the links, in the hint style; the refused links get none.
        let screen = s.d.screen();
        assert!(
            screen.contains("afirst") && screen.contains("ssecond") && screen.contains("dhttps://three"),
            "{at}: {screen}"
        );
        assert!(screen.contains("a click b") && screen.contains("c f d"), "{at}: {screen}");
        let buf = s.d.buffer();
        let hint = s.d.app.theme.hint();
        let (x, y) = cell_of(&s, "afirst");
        assert_eq!(buf[(x, y)].style().bg, hint.bg, "{at}");
        assert!(buf[(x, y)].modifier.contains(ratatui::style::Modifier::BOLD), "{at}");
        // Refused links are plain text: not underlined as web links are.
        let (x, y) = cell_of(&s, "click");
        assert!(!buf[(x, y)].modifier.contains(ratatui::style::Modifier::UNDERLINED), "{at}");
        let (x, y) = cell_of(&s, "first");
        assert!(buf[(x + 1, y)].modifier.contains(ratatui::style::Modifier::UNDERLINED), "{at}");
        let pick = s.d.app.i18n.label(Label::HintPickLink).to_string();
        let cancel = s.d.app.i18n.label(Label::HintCancel).to_string();
        assert!(s.d.status_line().contains(&format!("o {pick} · Esc {cancel}")), "{at}: {}", s.d.status_line());
        press(&mut s, "s");
        assert_eq!(opened(&mut s), ["https://two.example.net/2"], "{at}");
        assert!(s.d.app.picker().is_none(), "{at}");
        assert!(!s.d.screen().contains("ssecond"), "{at}: the letters are gone");
    }
}

#[test]
fn esc_or_a_key_that_picks_nothing_stops_picking_and_opens_nothing() {
    let mut s = shown(&[MIXED], 80, 24, Lang::En);
    press(&mut s, "o");
    press(&mut s, "esc");
    assert!(s.d.app.picker().is_none() && opened(&mut s).is_empty());
    press(&mut s, "s");
    assert!(opened(&mut s).is_empty(), "no longer picking");
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
        s.d.screen();
        let (x, y) = cell_of(&s, "second");
        s.d.mouse(MouseEventKind::Down(MouseButton::Left), x + 2, y);
        assert_eq!(opened(&mut s), ["https://two.example.net/2"], "{at}");
        s.d.screen();
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
        assert!(s.d.screen().contains("dhttps://"), "{at}: {}", s.d.screen());
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
fn past_nine_links_each_gets_two_letters_and_both_pick_it() {
    let text: String = (0..12).map(|i| format!("<https://h{i}.example.com/|l{i}> ")).collect();
    let mut s = shown(&[text.as_str()], 120, 40, Lang::En);
    press(&mut s, "o");
    let hints: Vec<String> = s.d.app.picker().unwrap().picks.iter().map(|k| k.hint.clone()).collect();
    assert_eq!(hints.len(), 12);
    assert!(hints.iter().all(|h| h.len() == 2), "{hints:?}");
    press(&mut s, "a");
    assert!(opened(&mut s).is_empty() && s.d.app.picker().is_some(), "one letter is not a pick yet");
    press(&mut s, "s");
    assert_eq!(opened(&mut s), ["https://h1.example.com/"]);
}
