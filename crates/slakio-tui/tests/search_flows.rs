//! Searching messages (`/`, `Space s`, `:search`) and reading from a day (`:jump <date>`): the
//! words are searched on `Enter` or once typing pauses, never per key nor twice; results show
//! newest first with the words found marked and every remote text sanitised; `Enter` opens a
//! result as a Slack link opens (a message read around and selected, a reply's thread beside);
//! pages and workspaces; `Esc` drops what is on its way; a failure is said in the popup. A day
//! reads the focused conversation from its first message. At 120x40 and 80x24, in English and
//! Korean.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use ratatui::crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyModifiers};
use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, FailKind, Found, Hit, Request};
use slakio_core::i18n::{I18n, Label, Lang, Msg};
use slakio_core::model::{Target, Ts, UserId};
use slakio_core::sanitize::Remote;
use slakio_tui::app::Overlay;
use slakio_tui::app::dialog::Question;
use slakio_tui::app::model::SignedIn;
use slakio_tui::app::query::PaneKind;
use slakio_tui::app::search::PAUSE;
use slakio_world::{World, names};
use std::time::Duration;

const SIZES: [(u16, u16, Lang); 3] = [(120, 40, Lang::En), (80, 24, Lang::En), (80, 24, Lang::Ko)];

fn conversation(name: &str) -> Target {
    let w = World::demo();
    let s = w.snapshot();
    let c = s.conversations.iter().find(|c| c.workspace == s.workspaces[0].id && c.name == name).unwrap();
    Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() }
}

/// Booted, both workspaces signed in on their hosts, at `w`×`h` in `lang`; nothing open.
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

/// Type `text` key by key, then one turn of the loop (the scripted backend's).
fn type_words(s: &mut Scripted, text: &str) {
    for c in text.chars() {
        let ev = TermEvent::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        s.d.app.handle_event(ev, s.d.now);
    }
    s.pump();
}

/// Let `by` pass: the app's clock ticks once.
fn wait(s: &mut Scripted, by: Duration) {
    s.d.now += by;
    s.d.app.on_tick(s.d.now);
    s.pump();
}

fn searches(s: &Scripted) -> Vec<(String, u32, String)> {
    (s.b.held.iter())
        .filter_map(|r| match &r.command {
            Command::Search { query, page } => {
                Some((query.clone(), *page, r.workspace.as_ref().map(|w| w.to_string()).unwrap_or_default()))
            }
            _ => None,
        })
        .collect()
}

fn search_index(s: &Scripted) -> usize {
    s.b.held.iter().position(|r| matches!(r.command, Command::Search { .. })).expect("a search asked")
}

fn has(screen: &str, words: &str) -> bool {
    screen.replace(' ', "").contains(&words.replace(' ', ""))
}

/// A hit of `target` at `ts`.
fn hit(target: &Target, ts: Ts, thread: Option<Ts>, text: &str) -> Hit {
    Hit {
        conversation: target.conversation().clone(),
        name: Remote::new("hostile\u{1b}[2Jname"),
        ts,
        thread,
        user: UserId::new("U-NOBODY"),
        author: Remote::new("\u{1b}]0;owned\u{7}mallory"),
        text: Remote::new(text),
        permalink: String::new(),
    }
}

/// Answer the held search with `hits` (one page of `pages`).
fn answer_with(s: &mut Scripted, hits: Vec<Hit>, page: u32, pages: u32) {
    let r: Request = s.b.held.remove(search_index(s));
    let Command::Search { query, .. } = r.command else { unreachable!() };
    let total = hits.len() as u32 * pages;
    let ws = r.workspace.unwrap();
    let found = Found { workspace: ws, query, page, pages, total, hits, users: Vec::new() };
    s.b.push(Envelope { generation: r.id, event: Event::Found(Box::new(found)) });
    s.pump();
}

#[test]
fn typing_searches_after_a_pause_or_on_enter_and_never_per_key_nor_twice() {
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        let i18n = I18n::new(lang);
        s.keys("/");
        assert_eq!(s.d.app.overlay(), Some(Overlay::Search), "{w}x{h}");
        assert!(has(&s.d.screen(), &i18n.label(Label::SearchPlaceholder).chars().take(10).collect::<String>()));
        type_words(&mut s, "dep");
        wait(&mut s, PAUSE / 2);
        type_words(&mut s, "loy");
        assert!(searches(&s).is_empty(), "nothing per key");
        wait(&mut s, PAUSE / 2);
        assert!(searches(&s).is_empty(), "the pause starts again with each key");
        wait(&mut s, PAUSE);
        let ws0 = World::demo().snapshot().workspaces[0].id.to_string();
        assert_eq!(searches(&s), [("deploy".to_string(), 1, ws0.clone())]);
        // Enter on the words on their way asks nothing more; a long pause neither.
        s.keys("enter");
        wait(&mut s, PAUSE * 3);
        assert_eq!(searches(&s).len(), 1);
        s.b.answer(search_index(&s));
        s.pump();
        let found = s.d.app.search_found().expect("results").clone();
        assert!(!found.hits.is_empty() && found.page == 1 && found.pages > 1);
        let screen = s.d.screen();
        assert!(has(&screen, "deploy"), "{w}x{h}: {screen}");
        let count = i18n.msg(&Msg::SearchCount {
            total: found.total.to_string(),
            page: "1".into(),
            pages: found.pages.to_string(),
        });
        assert!(has(&screen, &count), "{w}x{h}: {screen}");
        // Enter right after typing searches at once.
        type_words(&mut s, " in:#backend");
        s.keys("enter");
        assert_eq!(searches(&s), [("deploy in:#backend".to_string(), 1, ws0)]);
        s.b.held.clear();
    }
}

#[test]
fn enter_opens_a_message_read_around_it_and_a_reply_s_thread_beside() {
    let big = conversation(names::BIG_HISTORY);
    let threads = conversation(names::LONG_THREADS);
    let world = World::demo();
    let old = world.message(big.conversation(), 1000).unwrap().ts;
    let parent = (0..world.message_count(threads.conversation()))
        .filter_map(|i| world.message(threads.conversation(), i))
        .find(|m| m.thread.is_some_and(|t| t.replies >= 3))
        .unwrap();
    let reply = world.thread(threads.conversation(), parent.ts, None, 5).unwrap().0[2].ts;
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        s.keys("space s");
        type_words(&mut s, "deploy");
        s.keys("enter");
        let text = "an \u{E000}old\u{E001} \u{1b}[31mdeploy\u{202E} <https://x.test/a|runbook> note";
        answer_with(&mut s, vec![hit(&big, old, None, text), hit(&threads, reply, Some(parent.ts), "a reply")], 1, 1);
        let screen = s.d.screen();
        demo::assert_harmless(&screen);
        assert!(has(&screen, "#big-history"), "the list's name first: {screen}");
        assert!(has(&screen, "mallory"), "{w}x{h}: {screen}");
        assert!(has(&screen, "runbook") && !screen.contains("<https"), "Slack's markup read: {screen}");
        // The first result: its conversation read around it, the message selected and marked.
        s.keys("enter");
        assert_eq!(s.d.app.overlay(), None);
        let around =
            s.b.held
                .iter()
                .position(|r| matches!(&r.command, Command::Around { target, at, .. } if *target == big && *at == old));
        s.b.answer(around.expect("read around the message"));
        s.pump();
        let p = s.d.app.focused_pane().expect("the conversation has the keyboard");
        assert_eq!((p.target(), p.flashed()), (&big, Some(old)), "{w}x{h}");
        assert!(s.b.held.is_empty(), "nothing more: {:?}", s.b.held);
        // The reply: its thread beside the conversation, the reply selected.
        s.keys("/");
        assert!(s.d.app.search_found().is_some(), "the results are kept");
        s.keys("down");
        s.keys("enter");
        for _ in 0..4 {
            if s.b.held.is_empty() {
                break;
            }
            s.b.answer(0);
            s.pump();
        }
        let thread = s.d.app.focused_pane().expect("the thread has the keyboard");
        assert_eq!(thread.kind(), PaneKind::Thread, "{w}x{h}");
        assert_eq!(thread.flashed(), Some(reply));
        assert!(s.d.app.pane_showing(&threads).is_some(), "the conversation beside it");
    }
}

#[test]
fn pages_and_workspaces_are_asked_one_at_a_time_and_esc_drops_what_is_on_its_way() {
    let big = conversation(names::BIG_HISTORY);
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        s.keys("/");
        type_words(&mut s, "the");
        s.keys("enter");
        let hits: Vec<Hit> = (0..20).map(|i| hit(&big, Ts(1_000_000 * (100 - i)), None, "x")).collect();
        answer_with(&mut s, hits.clone(), 1, 3);
        s.keys("pageup");
        assert!(searches(&s).is_empty(), "no page before the first");
        s.keys("pagedown");
        assert_eq!(searches(&s).iter().map(|q| q.1).collect::<Vec<_>>(), [2]);
        s.keys("pagedown");
        assert_eq!(searches(&s).len(), 1, "page 2 is on its way: asked once");
        answer_with(&mut s, hits.clone(), 2, 3);
        assert_eq!(s.d.app.search_found().map(|f| f.page), Some(2));
        s.keys("pagedown");
        assert_eq!(searches(&s).iter().map(|q| q.1).collect::<Vec<_>>(), [3]);
        answer_with(&mut s, hits.clone(), 3, 3);
        s.keys("pagedown");
        assert!(searches(&s).is_empty(), "no page after the last");
        // The next workspace: searched at once, about it.
        s.keys("tab");
        let ws1 = World::demo().snapshot().workspaces[1].id.to_string();
        assert_eq!(searches(&s), [("the".to_string(), 1, ws1)]);
        assert!(s.d.app.search_found().is_none(), "the other workspace's results go");
        // Esc: the answer on its way is never shown.
        s.keys("esc");
        assert_eq!(s.d.app.overlay(), None);
        s.b.answer(search_index(&s));
        s.pump();
        assert!(s.d.app.search_found().is_none() && !s.d.app.searching(), "{w}x{h}");
        s.keys("/");
        assert!(s.d.app.search_found().is_none());
        assert_eq!(s.d.app.search_query(), "the", "the words stay");
        s.keys("ctrl+u");
        assert_eq!(s.d.app.search_query(), "");
        s.keys("ctrl+c");
        assert!(!s.d.app.searching());
    }
}

#[test]
fn a_failed_search_is_said_in_the_popup_and_enter_tries_again() {
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        let i18n = I18n::new(lang);
        s.keys("/");
        type_words(&mut s, "deploy");
        s.keys("enter");
        s.b.fail(search_index(&s), FailKind::RateLimited { retry_after: Some(Duration::from_secs(7)) });
        s.pump();
        let screen = s.d.screen();
        let why = i18n.label(Label::FailRateLimited).to_string();
        assert!(has(&screen, &why.chars().take(12).collect::<String>()), "{w}x{h}: {screen}");
        assert!(has(&screen, &i18n.label(Label::SearchRetry)), "{w}x{h}: {screen}");
        s.keys("enter");
        assert_eq!(searches(&s).len(), 1, "asked again on Enter only");
        s.b.answer(search_index(&s));
        s.pump();
        assert!(s.d.app.search_failed_kind().is_none());
        // Nothing found says so.
        type_words(&mut s, "zzqqxx");
        s.keys("enter");
        s.b.answer(search_index(&s));
        s.pump();
        assert!(has(&s.d.screen(), &i18n.label(Label::SearchNothing)), "{w}x{h}");
    }
}

#[test]
fn the_palette_searches_its_words_at_once() {
    let mut s = signed(120, 40, Lang::En);
    s.keys(":");
    type_words(&mut s, "search from:@alex rollback");
    s.keys("enter");
    assert_eq!(s.d.app.overlay(), Some(Overlay::Search));
    assert_eq!(searches(&s).len(), 1);
    assert_eq!(searches(&s)[0].0, "from:@alex rollback");
}

#[test]
fn a_day_reads_the_focused_conversation_from_its_first_message() {
    let big = conversation(names::BIG_HISTORY);
    let world = World::demo();
    let m = world.message(big.conversation(), 4000).unwrap();
    let day = slakio_tui::time::date(m.ts);
    let start = slakio_tui::when::start(slakio_tui::time::day(m.ts));
    let first = world.message(big.conversation(), world.first_at_or_after(big.conversation(), start)).unwrap().ts;
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        let i18n = I18n::new(lang);
        // Nothing focused: said, nothing asked.
        s.keys(":");
        type_words(&mut s, &format!("jump {day}"));
        s.keys("enter");
        assert!(s.b.held.is_empty());
        assert!(has(&s.d.screen(), &i18n.label(Label::JumpDateNone).chars().take(14).collect::<String>()));
        // The conversation open (a link to it), then the day.
        s.keys(":");
        type_words(&mut s, &format!("open https://acme.slack.com/archives/{}", big.conversation()));
        s.keys("enter");
        s.b.answer(0);
        s.pump();
        s.keys(":");
        type_words(&mut s, &format!("jump {day}"));
        s.keys("enter");
        let dated =
            s.b.held.iter().position(
                |r| matches!(&r.command, Command::Dated { target, at, .. } if *target == big && *at == start),
            );
        s.b.answer(dated.expect("read from the day"));
        s.pump();
        let p = s.d.app.focused_pane().unwrap();
        let sel = p.selected().map(|i| p.messages()[i].ts);
        assert_eq!(sel, Some(first), "{w}x{h}: the day's first message");
        assert_eq!(p.flashed(), Some(first));
        assert!(p.cut(), "the newer ones are far: not read");
        assert!(s.d.screen().contains(&day), "{w}x{h}: the day's row");
        // A day loaded already: selected at once, nothing asked.
        s.keys("k");
        s.keys(":");
        type_words(&mut s, &format!("jump {day}"));
        s.keys("enter");
        assert!(s.b.held.is_empty(), "{w}x{h}: {:?}", s.b.held);
        let p = s.d.app.focused_pane().unwrap();
        assert_eq!(p.selected().map(|i| p.messages()[i].ts), Some(first));
        // Not a day: said under the list, the palette stays, nothing asked.
        s.keys(":");
        type_words(&mut s, "jump someday");
        s.keys("enter");
        assert_eq!(s.d.app.overlay(), Some(Overlay::Palette));
        let bad = i18n.msg(&Msg::JumpDateBad { text: "someday".into() }).to_string();
        assert!(has(&s.d.screen(), &bad.chars().take(10).collect::<String>()), "{w}x{h}");
        assert!(s.b.held.is_empty());
        s.keys("esc");
        // `Space j` readies the command line for a day.
        s.keys("space j");
        assert_eq!(s.d.app.overlay(), Some(Overlay::Palette));
        s.keys("esc");
    }
}

#[test]
fn a_result_whose_conversation_the_workspace_does_not_list_is_offered_for_the_browser() {
    let mut s = signed(80, 24, Lang::En);
    s.keys("/");
    type_words(&mut s, "x");
    s.keys("enter");
    let ws = World::demo().snapshot().workspaces[0].id.clone();
    let other =
        Target::Conversation { workspace: ws, conversation: slakio_core::model::ConversationId::new("C0NOTLISTED") };
    let mut h = hit(&other, Ts(5_000_000), None, "x");
    h.permalink = "https://acme.slack.com/archives/C0NOTLISTED/p0000000005000000".into();
    let mut bad = h.clone();
    bad.permalink = "javascript:alert(1)".into();
    answer_with(&mut s, vec![h, bad], 1, 1);
    s.keys("enter");
    assert_eq!(s.d.app.question(), Some(Question::Browser));
    assert!(s.b.held.is_empty());
    s.keys("n");
    // A link that is not a web address is never offered.
    s.keys("/");
    s.keys("down");
    s.keys("enter");
    assert_eq!(s.d.app.question(), None);
    assert!(s.b.held.is_empty());
}
