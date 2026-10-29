//! Links opened in the browser: `o` opens the selected message's first link, `:open 2` its
//! second, `:open <address>` the address typed. Only a web address (`http`, `https`) is handed
//! to the binary (`Effect::OpenLink`; no test starts a browser), the host it goes to said; a link
//! whose text names another host is asked about first, the real host shown; `javascript:`,
//! `data:`, `file:`, `slack:` and the like are refused with a reason. Nothing goes to Slack, and
//! a read-only workspace opens links too. At 120x40 and 80x24, in English and Korean.

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
    s
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
fn open_zero_is_no_link_and_opens_nothing() {
    let mut s = shown(&["see <https://example.com/a> now"], 80, 24, Lang::En);
    command(&mut s, "open 0");
    assert!(opened(&mut s).is_empty());
    assert!(s.d.app.question().is_none());
    let none = s.d.app.i18n.label(Label::OpenNone).to_string();
    assert!(s.d.status_line().contains(&none), "{}", s.d.status_line());
    // The first link is still link 1.
    command(&mut s, "open 1");
    assert_eq!(opened(&mut s), ["https://example.com/a"]);
}

const SIZES: [(Lang, u16, u16); 3] = [(Lang::En, 120, 40), (Lang::En, 80, 24), (Lang::Ko, 80, 24)];

#[test]
fn o_opens_the_selected_messages_first_link_and_open_2_its_second_saying_the_host() {
    for (lang, w, h) in SIZES {
        let at = format!("{lang:?} {w}x{h}");
        let i18n = I18n::new(lang);
        let text = "see <https://docs.example.com/runbooks/deploy|the runbook> or <https://status.example.com/x?y=1>";
        let mut s = shown(&[text], w, h, lang);
        let asked = s.b.log.len() + s.b.held.len();
        s.keys("o");
        assert_eq!(opened(&mut s), ["https://docs.example.com/runbooks/deploy"], "{at}");
        let said = i18n.msg(&slakio_core::i18n::Msg::OpenOpening { host: "docs.example.com".into() }).to_string();
        assert!(s.d.status_line().contains(&said), "{at}: {}", s.d.status_line());
        command(&mut s, "open 2");
        assert_eq!(opened(&mut s), ["https://status.example.com/x?y=1"], "{at}");
        command(&mut s, "open 3");
        assert!(opened(&mut s).is_empty(), "{at}");
        assert!(s.d.status_line().contains(&i18n.label(Label::OpenNone).to_string()), "{at}: {}", s.d.status_line());
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
        // Text that names the same host, or no host at all, opens at once.
        let mut s =
            shown(&["<https://docs.example.com/a|docs.example.com> <https://docs.example.com/b|here>"], w, h, lang);
        s.keys("o");
        assert_eq!(opened(&mut s), ["https://docs.example.com/a"], "{at}");
        command(&mut s, "open 2");
        assert_eq!(opened(&mut s), ["https://docs.example.com/b"], "{at}");
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
        s.keys("o");
        let screen = s.d.screen();
        let dialog = dialog_rows(&screen, &s.d.app.i18n.label(Label::DialogOpenTitle));
        assert!(dialog.contains("zz.evil.test"), "{at}: the real host's end: {dialog}");
        assert!(!dialog.contains("docs.google.com.x"), "{at}: the host's start: {dialog}");
        // The real host comes before what the link's text says, which is there.
        let (host_at, said_at) = (dialog.find("evil.test").unwrap(), dialog.find("docs.google.com/x"));
        assert!(said_at.is_some_and(|said| host_at < said), "{at}: {dialog}");
        s.keys("n");
        // Without text of its own, the status line names the host's end.
        command(&mut s, "open 2");
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
