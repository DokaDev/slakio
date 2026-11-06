//! The edges of Slack links opened inside slakio: a link that names a workspace opens only in
//! that one (an Enterprise Grid organization's link in any workspace that lists it); a link
//! opened again while its message is being read is not asked twice; a link whose message is
//! read no more (`G`, its pane closed) never asks about the browser later, and only Slack's "not
//! here" errors do; a reply far back in a long thread is read back to; a message that is gone is
//! said, never a neighbour marked; the mark goes after its moment even when the link's page came
//! while another tab was shown; a cut view takes older pages but no live message after its end;
//! the picker names a reply's link; the cut notice is whole at 50x24 in Korean; every link of a
//! long message gets its letters.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, FailKind, Generation, Live};
use slakio_core::i18n::{I18n, Label, Lang, Msg};
use slakio_core::model::{Message, Target, Ts};
use slakio_core::sanitize::Remote;
use slakio_tui::app::dialog::Question;
use slakio_tui::app::model::SignedIn;
use slakio_tui::app::query::PaneKind;
use slakio_world::{LONG_THREAD_REPLIES, World, names};
use std::time::Duration;

const SIZES: [(u16, u16, Lang); 3] = [(120, 40, Lang::En), (80, 24, Lang::En), (80, 24, Lang::Ko)];

fn conversation(name: &str) -> Target {
    let w = World::demo();
    let s = w.snapshot();
    let c = s.conversations.iter().find(|c| c.workspace == s.workspaces[0].id && c.name == name).unwrap();
    Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() }
}

fn message(target: &Target, i: usize) -> Message {
    World::demo().message(target.conversation(), i).unwrap()
}

fn link(host: &str, target: &Target, ts: Ts) -> String {
    format!("https://{host}/archives/{}/p{:016}", target.conversation(), ts.0)
}

/// Booted, the first workspace signed in on `acme.slack.com`, the second on
/// `other-co.slack.com`, at `w`×`h` in `lang`; nothing open.
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

fn command(s: &mut Scripted, text: &str) {
    s.keys(":");
    s.d.type_text(text);
    s.keys("enter");
}

fn selected(s: &Scripted) -> Option<Ts> {
    let p = s.d.app.focused_pane()?;
    p.selected().map(|i| p.messages()[i].ts)
}

fn has(screen: &str, words: &str) -> bool {
    screen.replace(' ', "").contains(&words.replace(' ', ""))
}

fn arounds(s: &Scripted) -> usize {
    s.b.held.iter().filter(|r| matches!(r.command, Command::Around { .. })).count()
}

#[test]
fn a_link_that_names_a_workspace_opens_only_there() {
    let deploys = conversation("deploys");
    let at = message(&deploys, 3).ts;
    let other_team = World::demo().snapshot().workspaces[1].id.clone();
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        // A host not signed in to: the browser is offered, though a workspace signed in to lists
        // the conversation.
        command(&mut s, &format!("open {}", link("stranger.slack.com", &deploys, at)));
        assert_eq!(s.d.app.question(), Some(Question::Browser), "{w}x{h}");
        let why = &s.d.app.offered().unwrap().why;
        assert_eq!(why, &Msg::JumpElsewhere { host: "stranger.slack.com".into() });
        s.keys("n");
        // A host signed in to that does not list it: not listed there, whoever else does.
        command(&mut s, &format!("open {}", link("other-co.slack.com", &deploys, at)));
        assert_eq!(s.d.app.question(), Some(Question::Browser), "{w}x{h}");
        assert_eq!(s.d.app.offered().unwrap().why, Msg::JumpUnlisted { host: "other-co.slack.com".into() });
        s.keys("n");
        // The web client's link naming the other workspace by its id: the same.
        command(&mut s, &format!("open https://app.slack.com/client/{other_team}/{}", deploys.conversation()));
        assert_eq!(s.d.app.question(), Some(Question::Browser), "{w}x{h}");
        assert!(matches!(s.d.app.offered().unwrap().why, Msg::JumpUnlisted { .. }));
        s.keys("n");
        assert!(s.b.held.is_empty() && s.d.app.focused_pane().is_none(), "{w}x{h}: nothing opened, nothing sent");
        // An Enterprise Grid organization's link opens in the workspace that lists it.
        command(&mut s, &format!("open {}", link("acme-org.enterprise.slack.com", &deploys, at)));
        assert!(s.d.app.question().is_none(), "{w}x{h}");
        assert_eq!(s.d.app.focused_pane().map(|p| p.target().clone()), Some(deploys.clone()));
    }
}

#[test]
fn a_link_opened_again_while_its_message_is_read_is_not_asked_twice() {
    let big = conversation(names::BIG_HISTORY);
    let at = message(&big, 1000).ts;
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        let url = link("acme.slack.com", &big, at);
        command(&mut s, &format!("open {url}"));
        command(&mut s, &format!("open {url}"));
        command(&mut s, &format!("open {url}"));
        assert_eq!(s.b.held.len(), 1, "{w}x{h}: one request: {:?}", s.b.held);
        s.b.answer(0);
        s.pump();
        assert_eq!(selected(&s), Some(at), "{w}x{h}");
        assert_eq!(s.d.app.focused_pane().unwrap().flashed(), Some(at));
        // Its failure is still the link's: a refusal offers the browser.
        let other = message(&big, 500).ts;
        command(&mut s, &format!("open {}", link("acme.slack.com", &big, other)));
        command(&mut s, &format!("open {}", link("acme.slack.com", &big, other)));
        assert_eq!(arounds(&s), 1, "{w}x{h}");
        s.b.fail(0, FailKind::Api { code: "channel_not_found".into() });
        s.pump();
        assert_eq!(s.d.app.question(), Some(Question::Browser), "{w}x{h}");
        s.keys("n");
    }
}

#[test]
fn a_link_read_no_more_never_offers_the_browser_and_only_not_here_codes_do() {
    let big = conversation(names::BIG_HISTORY);
    let at = message(&big, 1000).ts;
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        let url = link("acme.slack.com", &big, at);
        // `G` while it is read: what comes of it is asked about no more.
        command(&mut s, &format!("open {url}"));
        s.keys("G");
        let i = s.b.held.iter().position(|r| matches!(r.command, Command::Around { .. })).unwrap();
        s.b.fail(i, FailKind::Api { code: "channel_not_found".into() });
        s.pump();
        assert!(s.d.app.question().is_none(), "{w}x{h}: after G");
        s.b.held.clear();
        // Its pane closed while it is read: the same.
        command(&mut s, &format!("open {}", link("acme.slack.com", &big, message(&big, 900).ts)));
        command(&mut s, "q");
        assert!(s.d.app.focused_pane().is_none(), "{w}x{h}: closed");
        let i = s.b.held.iter().position(|r| matches!(r.command, Command::Around { .. })).unwrap();
        s.b.fail(i, FailKind::Api { code: "not_in_channel".into() });
        s.pump();
        assert!(s.d.app.question().is_none(), "{w}x{h}: after closing");
        s.b.held.clear();
        // Slack's own trouble is no reason to go to the browser.
        command(&mut s, &format!("open {}", link("acme.slack.com", &big, message(&big, 800).ts)));
        let i = s.b.held.iter().position(|r| matches!(r.command, Command::Around { .. })).unwrap();
        s.b.fail(i, FailKind::Api { code: "internal_error".into() });
        s.pump();
        assert!(s.d.app.question().is_none(), "{w}x{h}: internal_error");
        s.b.held.clear();
        // "Not here" is.
        command(&mut s, &format!("open {}", link("acme.slack.com", &big, message(&big, 700).ts)));
        let i = s.b.held.iter().position(|r| matches!(r.command, Command::Around { .. })).unwrap();
        s.b.fail(i, FailKind::Api { code: "not_in_channel".into() });
        s.pump();
        assert_eq!(s.d.app.question(), Some(Question::Browser), "{w}x{h}");
    }
}

#[test]
fn a_reply_far_back_in_a_long_thread_is_read_back_to_and_selected() {
    let threads = conversation(names::LONG_THREADS);
    let world = World::demo();
    let parent = message(&threads, 0);
    assert_eq!(parent.thread.map(|t| t.replies), Some(LONG_THREAD_REPLIES));
    let (replies, complete) = world.thread(threads.conversation(), parent.ts, None, 2000).unwrap();
    assert!(complete);
    // The first message of a thread is its own; the sixth reply after it, far older than the
    // newest page of the thread.
    let reply = replies[6].ts;
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        let url = format!(
            "{}?thread_ts={}&cid={}",
            link("acme.slack.com", &threads, reply),
            parent.ts,
            threads.conversation()
        );
        command(&mut s, &format!("open {url}"));
        let mut asked = 0;
        while !s.b.held.is_empty() {
            asked += 1;
            assert!(asked < 200, "{w}x{h}: reading back never ends");
            // Until the reply is loaded, nothing is marked: never a neighbour.
            let marked = s.d.pane(PaneKind::Thread).and_then(|p| p.flashed());
            assert!(marked.is_none_or(|m| m == reply), "{w}x{h}: {marked:?}");
            s.b.answer(0);
            s.pump();
        }
        let thread = s.d.app.focused_pane().expect("the thread has the keyboard");
        assert_eq!(thread.kind(), PaneKind::Thread, "{w}x{h}");
        assert_eq!(selected(&s), Some(reply), "{w}x{h}");
        assert_eq!(s.d.app.focused_pane().unwrap().flashed(), Some(reply));
        assert!(!s.d.status_line().contains(&I18n::new(lang).label(Label::JumpGone).to_string()));
    }
}

#[test]
fn a_message_that_is_gone_is_said_and_no_neighbour_is_marked() {
    let deploys = conversation("deploys");
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        s.d.list_cursor_on("deploys");
        s.keys("enter");
        s.b.answer(0);
        s.pump();
        let loaded = s.d.app.pane_showing(&deploys).unwrap().messages().to_vec();
        // Between two loaded messages: gone (deleted) since the link was made.
        let gone = Ts(loaded[loaded.len() - 3].ts.0 + 1);
        command(&mut s, &format!("open {}", link("acme.slack.com", &deploys, gone)));
        assert!(s.b.held.is_empty(), "{w}x{h}: {:?}", s.b.held);
        let p = s.d.app.focused_pane().unwrap();
        assert_eq!(p.flashed(), None, "{w}x{h}: no neighbour marked");
        assert_eq!(selected(&s), Some(loaded[loaded.len() - 2].ts), "{w}x{h}: the nearest selected");
        let said = I18n::new(lang).label(Label::JumpGone).to_string();
        assert!(has(&s.d.status_line(), &said.chars().take(10).collect::<String>()), "{w}x{h}: {}", s.d.status_line());
    }
}

#[test]
fn the_mark_goes_after_its_moment_when_its_page_came_while_another_tab_was_shown() {
    let big = conversation(names::BIG_HISTORY);
    let at = message(&big, 1000).ts;
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        command(&mut s, &format!("open {}", link("acme.slack.com", &big, at)));
        let around = s.b.held.iter().position(|r| matches!(r.command, Command::Around { .. })).unwrap();
        let around = s.b.reply(around);
        // Another tab shown while it is read.
        s.d.open_in_tab("deploys");
        s.pump();
        while !s.b.held.is_empty() {
            s.b.answer(0);
            s.pump();
        }
        s.b.push(around);
        s.pump();
        // Back to the link's tab: it lands there now, marked from now.
        s.keys("space 1");
        assert_eq!(s.d.app.focused_pane().map(|p| p.target().clone()), Some(big.clone()), "{w}x{h}");
        assert_eq!(s.d.app.focused_pane().unwrap().flashed(), Some(at), "{w}x{h}");
        s.d.now += Duration::from_secs(3);
        assert!(s.d.app.on_tick(s.d.now), "{w}x{h}");
        assert_eq!(s.d.app.focused_pane().unwrap().flashed(), None, "{w}x{h}: the mark went");
    }
}

#[test]
fn a_cut_view_takes_older_pages_but_no_live_message_after_its_end() {
    let big = conversation(names::BIG_HISTORY);
    let at = message(&big, 1000).ts;
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        command(&mut s, &format!("open {}", link("acme.slack.com", &big, at)));
        s.b.answer(0);
        s.pump();
        let before = s.d.app.focused_pane().unwrap().messages().len();
        // A live message (the newest of all) does not go after the cut end.
        let newest = message(&big, World::demo().message_count(big.conversation()) - 1);
        let live = Live::Message {
            workspace: big.workspace().clone(),
            conversation: big.conversation().clone(),
            thread: None,
            broadcast: false,
            changed: false,
            mention: false,
            message: Message { ts: Ts(newest.ts.0 + 1_000_000), ..newest },
            users: Vec::new(),
        };
        s.b.push(Envelope { generation: Generation::PUSHED, event: Event::Live(Box::new(live)) });
        s.pump();
        let p = s.d.app.focused_pane().unwrap();
        assert_eq!((p.messages().len(), p.messages().last().map(|m| m.ts)), (before, Some(at)), "{w}x{h}");
        // An older page goes before what is loaded; the view is still cut and says so.
        s.keys("g g");
        let older = s.b.held.iter().position(|r| matches!(r.command, Command::History { before: Some(_), .. }));
        s.b.answer(older.expect("an older page asked for"));
        s.pump();
        let p = s.d.app.focused_pane().unwrap();
        assert!(p.messages().len() > before && p.cut(), "{w}x{h}");
        assert_eq!(p.messages().last().map(|m| m.ts), Some(at));
        s.keys("G");
        s.b.held.clear();
    }
}

#[test]
fn the_picker_names_a_reply_link_apart_from_a_message_link() {
    let deploys = conversation("deploys");
    let (parent, reply) = (message(&deploys, 3).ts, message(&deploys, 4).ts);
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        s.d.list_cursor_on("deploys");
        s.keys("enter");
        let mut e = s.b.reply(0);
        let Event::History(page) = &mut e.event else { panic!("a page") };
        let author = page.messages[0].user.clone();
        let reply_link = format!("{}?thread_ts={parent}", link("acme.slack.com", &deploys, reply));
        let text = format!("see <{}> and <{reply_link}>", link("acme.slack.com", &deploys, parent));
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
        s.keys("o");
        s.keys("o");
        let screen = s.d.screen();
        let i18n = I18n::new(lang);
        assert!(has(&screen, &i18n.label(Label::OpenSlackMessage)), "{w}x{h}: {screen}");
        assert!(has(&screen, &i18n.label(Label::OpenSlackReply)), "{w}x{h}: {screen}");
        s.keys("escape");
    }
}

#[test]
fn the_cut_notice_is_whole_at_50x24_in_korean() {
    let big = conversation(names::BIG_HISTORY);
    let at = message(&big, 1000).ts;
    for lang in [Lang::Ko, Lang::En] {
        let mut s = signed(50, 24, lang);
        command(&mut s, &format!("open {}", link("acme.slack.com", &big, at)));
        s.b.answer(0);
        s.pump();
        let screen = s.d.screen();
        let i18n = I18n::new(lang);
        let keys = i18n.msg(&Msg::PaneNewerKeys { keys: "G".into() }).to_string();
        let words = format!("{} · {keys}", i18n.label(Label::PaneNewerCut));
        // Every word of it is on the screen.
        for word in words.split(' ') {
            assert!(screen.contains(word), "{lang:?}: {word:?} missing: {screen}");
        }
    }
}

#[test]
fn every_link_of_a_long_message_gets_its_letters() {
    let deploys = conversation("deploys");
    for (w, h, lang) in SIZES {
        let mut s = signed(w, h, lang);
        s.d.list_cursor_on("deploys");
        s.keys("enter");
        let mut e = s.b.reply(0);
        let Event::History(page) = &mut e.event else { panic!("a page") };
        let author = page.messages[0].user.clone();
        let words = "and some words to push the last link to the end of a line ".repeat(3);
        let text = format!(
            "links: <{}|first> <https://a.example.com/x|second> {words} <https://example.com/very/long/path/that/goes/on/and/on>",
            link("acme.slack.com", &deploys, message(&deploys, 2).ts)
        );
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
        s.keys("o");
        s.d.screen();
        let picks = s.d.app.picker().expect("picking").picks.len();
        assert_eq!(picks, 3);
        let hint = s.d.app.theme.hint();
        let lettered =
            s.d.buffer().content().iter().filter(|c| c.bg == hint.bg.unwrap_or_default() && c.symbol() != " ").count();
        assert_eq!(lettered, picks, "{w}x{h}: one letter on each link\n{}", s.d.screen());
        s.keys("escape");
    }
}
