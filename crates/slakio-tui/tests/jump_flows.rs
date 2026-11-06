//! Slack's own links opened inside slakio: a message's link (typed with `:open`, picked with
//! `o`) opens its conversation and selects that message, marked for a moment; one not loaded is
//! read around it (one request, nothing more), and when the newer messages were not read with
//! it the pane says so and `G` reads the newest; a reply's link opens its thread beside the
//! conversation with the reply selected. A link to a workspace not signed in to, to a
//! conversation the workspace does not list, or one Slack refuses to read, is offered for the
//! browser (the safe answer focused). The picker marks a Slack link `↳` and names it. At 120x40
//! and 80x24, in English and Korean.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use scripted::Scripted;
use slakio_core::backend::{Command, Event, FailKind};
use slakio_core::i18n::{I18n, Label, Lang, Msg};
use slakio_core::model::{Message, Target, Ts};
use slakio_core::sanitize::Remote;
use slakio_tui::app::Effect;
use slakio_tui::app::dialog::Question;
use slakio_tui::app::model::SignedIn;
use slakio_tui::app::query::PaneKind;
use slakio_world::{World, names};
use std::time::Duration;

const SIZES: [(u16, u16, Lang); 3] = [(120, 40, Lang::En), (80, 24, Lang::En), (80, 24, Lang::Ko)];

/// The first workspace's conversation `name`.
fn conversation(name: &str) -> Target {
    let w = World::demo();
    let s = w.snapshot();
    let c = s.conversations.iter().find(|c| c.workspace == s.workspaces[0].id && c.name == name).unwrap();
    Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() }
}

/// The world's message `i` (oldest first) of `target`.
fn message(target: &Target, i: usize) -> Message {
    World::demo().message(target.conversation(), i).unwrap()
}

/// A message link of Slack's to `ts` of `target` on `host`.
fn link(host: &str, target: &Target, ts: Ts) -> String {
    format!("https://{host}/archives/{}/p{:016}", target.conversation(), ts.0)
}

/// Booted, every workspace signed in on its host (`acme.slack.com`, `other-co.slack.com`), at
/// `w`×`h` in `lang`; nothing open.
fn signed(w: u16, h: u16, lang: Lang) -> Scripted {
    let mut s = Scripted::connected(true);
    s.d.app.i18n = I18n::new(lang);
    s.d.app.resize(w, h);
    let snapshot = World::demo().snapshot().clone();
    let hosts = ["acme.slack.com", "other-co.slack.com"];
    let signed: Vec<SignedIn> = (snapshot.workspaces.iter().zip(hosts))
        .map(|(w, host)| SignedIn {
            id: w.id.clone(),
            name: w.name.unsanitized().to_string(),
            read_only: true,
            host: host.to_string(),
            user: String::new(),
        })
        .collect();
    s.d.app.connect_workspaces(s.b.caps, &signed);
    s.pump();
    while !s.b.held.is_empty() {
        s.b.answer(0);
        s.pump();
    }
    s
}

/// `:open <address>`.
fn command(s: &mut Scripted, address: &str) {
    s.keys(":");
    s.d.type_text(&format!("open {address}"));
    s.keys("enter");
}

/// The addresses handed to the binary to open since the last call.
fn opened(s: &mut Scripted) -> Vec<String> {
    let effects = s.d.app.take_effects();
    effects.into_iter().filter_map(|e| if let Effect::OpenLink(u) = e { Some(u) } else { None }).collect()
}

/// The ts of the selected message of the focused pane.
fn selected(s: &Scripted) -> Option<Ts> {
    let p = s.d.app.focused_pane()?;
    p.selected().map(|i| p.messages()[i].ts)
}

/// A screen and the words compared with blanks left out (a wide character is drawn with one).
fn has(screen: &str, words: &str) -> bool {
    screen.replace(' ', "").contains(&words.replace(' ', ""))
}

#[test]
fn an_old_message_is_read_around_selected_and_marked_and_g_reads_the_newest() {
    let big = conversation(names::BIG_HISTORY);
    let at = message(&big, 1000).ts;
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        let i18n = I18n::new(lang);
        command(&mut s, &link("acme.slack.com", &big, at));
        // One request, around the message; nothing else asked.
        assert_eq!(s.b.held.len(), 1, "{w}x{h}");
        assert!(matches!(&s.b.held[0].command, Command::Around { target, at: a, .. } if *target == big && *a == at));
        s.b.answer(0);
        s.pump();
        assert!(s.b.held.is_empty(), "no more asked: {:?}", s.b.held);
        let p = s.d.app.focused_pane().expect("the conversation has the keyboard");
        assert_eq!(p.target(), &big);
        assert!(p.cut() && p.messages().last().unwrap().ts == at, "the newer ones are far: not read");
        assert_eq!(p.flashed(), Some(at));
        assert_eq!(selected(&s), Some(at));
        let screen = s.d.screen();
        let text = message(&big, 1000).text.unsanitized().to_string();
        assert!(has(&screen, &text.chars().take(12).collect::<String>()), "{w}x{h}: {screen}");
        assert!(has(&screen, &i18n.label(Label::PaneNewerCut)), "{w}x{h}: {screen}");
        // The mark goes after its moment.
        s.d.now += Duration::from_secs(3);
        assert!(s.d.app.on_tick(s.d.now));
        assert_eq!(s.d.app.focused_pane().unwrap().flashed(), None);
        // Older pages read back from it as ever.
        s.keys("g g");
        let older =
            |r: &slakio_core::backend::Request| matches!(r.command, Command::History { before: Some(b), .. } if b < at);
        assert!(matches!(&s.b.held[..], [r] if older(r)), "{w}x{h}: {:?}", s.b.held);
        s.b.held.clear();
        // `G` reads the newest again: the pane follows it.
        s.keys("G");
        let newest = (s.b.held.iter())
            .position(|r| matches!(&r.command, Command::History { target, before: None, .. } if *target == big));
        s.b.answer(newest.expect("the newest page asked for"));
        s.pump();
        let p = s.d.app.focused_pane().unwrap();
        assert!(!p.cut() && p.messages().last().unwrap().ts > at, "{w}x{h}");
        assert!(!has(&s.d.screen(), &i18n.label(Label::PaneNewerCut)));
    }
}

#[test]
fn a_message_loaded_already_is_selected_without_asking_anything() {
    let deploys = conversation("deploys");
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        s.d.list_cursor_on("deploys");
        s.keys("enter");
        s.b.answer(0);
        s.pump();
        let loaded =
            s.d.app
                .focused_pane()
                .map(|p| p.messages().to_vec())
                .unwrap_or_else(|| s.d.app.pane_showing(&deploys).unwrap().messages().to_vec());
        let at = loaded[loaded.len() - 3].ts;
        command(&mut s, &link("ACME.slack.com", &deploys, at));
        assert!(s.b.held.is_empty(), "{w}x{h}: {:?}", s.b.held);
        assert_eq!(selected(&s), Some(at));
        assert_eq!(s.d.app.focused_pane().unwrap().flashed(), Some(at));
        // The mark is drawn in the selection's gutter.
        s.d.screen();
        let buf = s.d.buffer();
        let hint = s.d.app.theme.hint();
        let marked = buf.content().iter().any(|c| c.symbol() == "▌" && c.bg == hint.bg.unwrap_or_default());
        assert!(marked, "{w}x{h}");
        assert!(opened(&mut s).is_empty());
    }
}

#[test]
fn a_reply_link_opens_its_thread_beside_with_the_reply_selected() {
    let threads = conversation(names::LONG_THREADS);
    let world = World::demo();
    let n = world.message_count(threads.conversation());
    let parent = (0..n)
        .rev()
        .filter_map(|i| world.message(threads.conversation(), i))
        .find(|m| m.thread.is_some_and(|t| t.replies >= 3))
        .unwrap();
    let (replies, _) = world.thread(threads.conversation(), parent.ts, None, 5).unwrap();
    let reply = replies[2].ts;
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        let url = format!(
            "{}?thread_ts={}&cid={}",
            link("acme.slack.com", &threads, reply),
            parent.ts,
            threads.conversation()
        );
        command(&mut s, &url);
        while !s.b.held.is_empty() {
            s.b.answer(0);
            s.pump();
        }
        let thread = s.d.app.focused_pane().expect("the thread has the keyboard");
        assert_eq!(thread.kind(), PaneKind::Thread, "{w}x{h}");
        assert_eq!(selected(&s), Some(reply));
        let conv = s.d.app.pane_showing(&threads).expect("the conversation beside it");
        assert_eq!(conv.selected().map(|i| conv.messages()[i].ts), Some(parent.ts));
        s.d.screen();
    }
}

#[test]
fn a_workspace_not_signed_in_to_or_a_conversation_not_listed_is_offered_for_the_browser() {
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        let i18n = I18n::new(lang);
        let elsewhere = "https://elsewhere.slack.com/archives/CNOTHERE1/p1700000000000100";
        command(&mut s, elsewhere);
        assert_eq!(s.d.app.question(), Some(Question::Browser), "{w}x{h}");
        let why = i18n.msg(&Msg::JumpElsewhere { host: "elsewhere.slack.com".into() }).to_string();
        let screen = s.d.screen();
        assert!(has(&screen, &why) || has(&screen, &why.chars().take(16).collect::<String>()), "{w}x{h}: {screen}");
        assert!(has(&screen, &i18n.label(Label::DialogBrowserTitle)), "{w}x{h}: {screen}");
        // Enter alone keeps it here; nothing went anywhere.
        s.keys("enter");
        assert!(s.d.app.question().is_none() && opened(&mut s).is_empty() && s.b.held.is_empty());
        // `y` opens it in the browser, as any web link.
        command(&mut s, elsewhere);
        s.keys("y");
        assert_eq!(opened(&mut s), [elsewhere]);
        assert!(s.b.held.is_empty() && s.b.log.iter().all(|r| !matches!(r.command, Command::Around { .. })));
        // A workspace signed in to that does not list the conversation.
        command(&mut s, "https://acme.slack.com/archives/CNOTHERE1");
        assert_eq!(s.d.app.question(), Some(Question::Browser));
        let why = i18n.msg(&Msg::JumpUnlisted { host: "acme.slack.com".into() }).to_string();
        assert!(has(&s.d.screen(), &why.chars().take(12).collect::<String>()), "{w}x{h}: {}", s.d.screen());
        s.keys("n");
        assert!(opened(&mut s).is_empty() && s.b.held.is_empty());
    }
}

#[test]
fn a_conversation_slack_refuses_to_read_is_offered_for_the_browser() {
    let general = conversation("general");
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        let url = link("acme.slack.com", &general, Ts(1_600_000_000_000_100));
        command(&mut s, &url);
        assert!(matches!(&s.b.held[..], [r] if matches!(r.command, Command::Around { .. })), "{w}x{h}");
        s.b.fail(0, FailKind::Api { code: "channel_not_found".into() });
        s.pump();
        assert_eq!(s.d.app.question(), Some(Question::Browser), "{w}x{h}");
        let refused = I18n::new(lang).label(Label::JumpRefused).to_string();
        assert!(has(&s.d.screen(), &refused.chars().take(12).collect::<String>()), "{w}x{h}: {}", s.d.screen());
        s.keys("y");
        assert_eq!(opened(&mut s), [url.as_str()]);
        // A failure for the network is said as ever, and offers nothing.
        command(&mut s, &link("acme.slack.com", &general, Ts(1_600_000_000_000_200)));
        let i = s.b.held.iter().position(|r| matches!(r.command, Command::Around { .. })).unwrap();
        s.b.fail(i, FailKind::Network);
        s.pump();
        assert!(s.d.app.question().is_none(), "{w}x{h}");
    }
}

#[test]
fn the_picker_marks_a_slack_link_and_picking_it_opens_it_here() {
    let deploys = conversation("deploys");
    let at = message(&deploys, 3).ts;
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        s.d.list_cursor_on("deploys");
        s.keys("enter");
        let mut e = s.b.reply(0);
        let Event::History(page) = &mut e.event else { panic!("a page") };
        let author = page.messages[0].user.clone();
        let text = format!("see <https://docs.example.com/a|the docs> and <{}>", link("acme.slack.com", &deploys, at));
        page.messages.push(Message {
            ts: Ts(page.messages.last().unwrap().ts.0 + 1_000_000),
            user: author,
            text: Remote::new(text),
            thread: None,
            reactions: Vec::new(),
            edited: false,
        });
        s.b.push(e);
        s.pump();
        s.keys("G");
        s.d.screen();
        // Letters on both links; `o` again lists them.
        s.keys("o");
        s.d.screen();
        s.keys("o");
        let screen = s.d.screen();
        let named = I18n::new(lang).label(Label::OpenSlackMessage).to_string();
        assert!(has(&screen, "↳ acme.slack.com") && has(&screen, &named), "{w}x{h}: {screen}");
        assert!(has(&screen, "docs.example.com"), "{w}x{h}: {screen}");
        // Its row picked: it opens here, not in the browser.
        s.keys("j enter");
        assert!(opened(&mut s).is_empty(), "{w}x{h}");
        let loaded = s.d.app.pane_showing(&deploys).unwrap().messages().iter().any(|m| m.ts == at);
        if loaded {
            assert!(s.b.held.is_empty() && selected(&s) == Some(at), "{w}x{h}");
        } else {
            assert!(matches!(&s.b.held[..], [r] if matches!(r.command, Command::Around { at: a, .. } if a == at)));
        }
    }
}
