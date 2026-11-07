//! The search popup and `:jump` past their first cut: pages only of the words shown, words
//! kept from before searched again on reopening (and replaced by typing), a search refused for
//! the sign-in signs the workspace out, a result's web link offered only when it is its own,
//! the keys, a reply's mark, short times, the bar on both lines, a wait said; `:jump` refuses a
//! day to come, says a quiet one, takes the language's own words, closes the popup it was run
//! over; the status line names the workspace in front.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use ratatui::crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyModifiers};
use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, FailKind, Found, Hit, Request};
use slakio_core::i18n::{I18n, Label, Lang, Msg};
use slakio_core::model::{ConversationId, Target, Ts, UserId, WorkspaceId};
use slakio_core::sanitize::Remote;
use slakio_tui::app::Overlay;
use slakio_tui::app::model::SignedIn;
use slakio_tui::app::search::PAUSE;
use slakio_tui::time::{self, Zone};
use slakio_world::{World, names};
use std::time::Duration;

const DAY: u64 = 86_400_000_000;

fn conversation(ws: usize, name: &str) -> Target {
    let w = World::demo();
    let s = w.snapshot();
    let c = s.conversations.iter().find(|c| c.workspace == s.workspaces[ws].id && c.name == name).unwrap();
    Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() }
}

fn workspace(i: usize) -> WorkspaceId {
    World::demo().snapshot().workspaces[i].id.clone()
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

fn type_words(s: &mut Scripted, text: &str) {
    for c in text.chars() {
        let ev = TermEvent::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        s.d.app.handle_event(ev, s.d.now);
    }
    s.pump();
}

fn wait(s: &mut Scripted, by: Duration) {
    s.d.now += by;
    s.d.app.on_tick(s.d.now);
    s.pump();
}

fn searches(s: &Scripted) -> Vec<(String, u32)> {
    (s.b.held.iter())
        .filter_map(|r| match &r.command {
            Command::Search { query, page } => Some((query.clone(), *page)),
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

fn hit(target: &Target, ts: Ts, thread: Option<Ts>, text: &str) -> Hit {
    Hit {
        conversation: target.conversation().clone(),
        name: Remote::new("general"),
        ts,
        thread,
        user: UserId::new("U-NOBODY"),
        author: Remote::new("mallory longname"),
        text: Remote::new(text),
        permalink: String::new(),
    }
}

/// Answer the held search with `hits`, page `page` of `pages`, `total` found.
fn answer_with(s: &mut Scripted, hits: Vec<Hit>, page: u32, pages: u32, total: u32) {
    let r: Request = s.b.held.remove(search_index(s));
    let Command::Search { query, .. } = r.command else { unreachable!() };
    let found = Found { workspace: r.workspace.unwrap(), query, page, pages, total, hits, users: Vec::new() };
    s.b.push(Envelope { generation: r.id, event: Event::Found(Box::new(found)) });
    s.pump();
}

/// Big-history of the first workspace open, through a link to it.
fn open_big(s: &mut Scripted) -> Target {
    let big = conversation(0, names::BIG_HISTORY);
    s.keys(":");
    type_words(s, &format!("open https://acme.slack.com/archives/{}", big.conversation()));
    s.keys("enter");
    s.b.answer(0);
    s.pump();
    big
}

fn dated(s: &Scripted) -> Option<(usize, Ts)> {
    s.b.held.iter().enumerate().find_map(|(i, r)| match &r.command {
        Command::Dated { at, .. } => Some((i, *at)),
        _ => None,
    })
}

/// What the popup says under the words, its lines joined.
fn popup_text(s: &Scripted) -> String {
    let b = s.d.app.search_box().expect("the popup");
    let buf = s.d.buffer();
    (b.list.y..b.list.bottom())
        .map(|y| (b.list.x..b.list.right()).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A clock a month after big-history's newest message (the demo's own clock is before it).
fn later() -> Ts {
    let w = World::demo();
    let big = conversation(0, names::BIG_HISTORY);
    Ts(w.message(big.conversation(), w.message_count(big.conversation()) - 1).unwrap().ts.0 + 30 * DAY)
}

fn notice(s: &Scripted) -> Option<Msg> {
    s.d.app.status.notice(s.d.now).map(|n| n.msg.clone())
}

#[test]
fn page_down_pages_only_the_words_shown_not_new_ones_on_their_way() {
    let big = conversation(0, names::BIG_HISTORY);
    let mut s = signed(120, 40, Lang::En);
    s.keys("/");
    type_words(&mut s, "foo");
    s.keys("enter");
    answer_with(&mut s, vec![hit(&big, Ts(5 * DAY), None, "foo")], 1, 5, 100);
    // New words on their way: PageDown does not page them with the old page count.
    type_words(&mut s, " bar");
    s.keys("enter");
    assert_eq!(searches(&s), [("foo bar".to_string(), 1)]);
    s.keys("pagedown");
    assert_eq!(searches(&s), [("foo bar".to_string(), 1)], "no page 2 of words never shown");
    s.b.held.clear();
    // New words typed and not searched yet: neither.
    type_words(&mut s, "z");
    s.keys("pagedown");
    assert!(searches(&s).is_empty());
}

#[test]
fn reopening_searches_words_closed_while_waiting_and_typing_replaces_kept_words() {
    let mut s = signed(80, 24, Lang::En);
    s.keys("/");
    type_words(&mut s, "deploy");
    wait(&mut s, PAUSE);
    assert_eq!(searches(&s).len(), 1);
    s.keys("esc");
    s.b.held.clear();
    s.keys("/");
    assert!(searches(&s).is_empty(), "not at once");
    wait(&mut s, PAUSE);
    assert_eq!(searches(&s), [("deploy".to_string(), 1)], "searched once the pause passed");
    s.b.answer(search_index(&s));
    s.pump();
    // Kept words: shown selected, and the first key typed replaces them.
    s.keys("esc");
    s.keys("/");
    assert!(s.d.app.search_fresh());
    type_words(&mut s, "x");
    assert_eq!(s.d.app.search_query(), "x");
    type_words(&mut s, "y");
    assert_eq!(s.d.app.search_query(), "xy", "then typing adds");
    s.keys("esc");
    s.keys("/");
    s.keys("backspace");
    assert_eq!(s.d.app.search_query(), "", "Backspace takes the kept words away");
}

#[test]
fn a_search_refused_for_the_sign_in_signs_its_workspace_out() {
    let mut s = signed(120, 40, Lang::En);
    let ws = workspace(0);
    assert!(!s.d.app.signed_out_in(&ws));
    s.keys("/");
    type_words(&mut s, "deploy");
    s.keys("enter");
    s.b.fail(search_index(&s), FailKind::Auth);
    s.pump();
    assert!(s.d.app.signed_out_in(&ws), "signed out, :login offered");
    assert!(s.d.app.search_failed_kind().is_some(), "and the popup says it failed");
}

#[test]
fn a_result_s_web_link_is_offered_only_when_it_is_its_own_on_its_workspace_s_host() {
    let ws = workspace(0);
    let other = Target::Conversation { workspace: ws, conversation: ConversationId::new("C0NOTLISTED") };
    for (link, offered) in [
        ("https://acme.slack.com/archives/C0NOTLISTED/p0000000005000000", true),
        ("https://other-co.slack.com/archives/C0NOTLISTED/p0000000005000000", false),
        ("https://evil.test/archives/C0NOTLISTED/p0000000005000000", false),
        ("https://acme.slack.com/archives/C0SOMEOTHER/p0000000005000000", false),
        ("https://acme.enterprise.slack.com/archives/C0NOTLISTED/p0000000005000000", true),
    ] {
        let mut s = signed(80, 24, Lang::En);
        s.keys("/");
        type_words(&mut s, "x");
        s.keys("enter");
        let mut h = hit(&other, Ts(5_000_000), None, "x");
        h.permalink = link.into();
        answer_with(&mut s, vec![h], 1, 1, 1);
        s.keys("enter");
        assert_eq!(s.d.app.question().is_some(), offered, "{link}");
        if !offered {
            let host = s.d.app.workspace_label(&workspace(0));
            assert_eq!(notice(&s), Some(Msg::JumpUnlisted { host }), "{link}: said, no address");
        }
        assert!(s.b.held.is_empty());
    }
}

#[test]
fn results_show_the_keys_a_reply_s_mark_short_times_and_the_bar_on_both_lines() {
    let big = conversation(0, names::BIG_HISTORY);
    let words = ["alpha", "bravo", "delta", "gamma", "kappa", "omega", "sigma", "theta", "zulus", "lemon"];
    let lead: String = words.iter().cycle().take(30).map(|w| format!("{w} ")).collect();
    for (w, h, lang) in
        [(120, 40, Lang::En), (80, 24, Lang::En), (80, 24, Lang::Ko), (50, 24, Lang::En), (50, 24, Lang::Ko)]
    {
        let mut s = signed(w, h, lang);
        let now = s.d.app.now_ts();
        let today = Ts(now.0 - 60_000_000);
        let old = Ts(now.0 - 400 * DAY);
        s.keys("/");
        type_words(&mut s, "deploy");
        s.keys("enter");
        let text = format!("{lead}\u{E000}deploy\u{E001} done");
        let hits =
            vec![hit(&big, today, Some(Ts(today.0 - 1)), &text), hit(&big, old, None, "x \u{E000}deploy\u{E001}")];
        answer_with(&mut s, hits, 1, 338, 6746);
        let screen = s.d.screen();
        let z = Zone::utc();
        // The keys that matter with results there: paging and the workspace.
        assert!(screen.contains("PgDn") && screen.contains("Tab"), "{w}x{h}: {screen}");
        // A reply is marked; today's time alone, an older one's date alone.
        assert!(screen.contains("#big-history ⤷"), "{w}x{h}: {screen}");
        assert!(has(&screen, &time::hm(today, &z)) && !has(&screen, &time::date(today, &z)), "{w}x{h}: {screen}");
        assert!(has(&screen, &time::date(old, &z)), "{w}x{h}: {screen}");
        // The author's name is not squeezed by the time.
        assert!(has(&screen, "mallory longname"), "{w}x{h}: {screen}");
        // The text before the word found is cut at a word.
        let line = screen.lines().find(|l| l.contains('…') && l.contains("deploy")).expect("a cut");
        let cut = line.split_once('…').map(|(_, r)| r.to_string()).unwrap_or_default();
        let first = cut.split_whitespace().next().unwrap_or_default();
        assert!(words.contains(&first) || first.starts_with("deploy"), "{w}x{h}: {cut:?}");
        // The selection bar is over both lines of the result under the cursor, not the next.
        let b = s.d.app.search_box().unwrap();
        let buf = s.d.buffer();
        let x = b.list.x + 1;
        let bar = buf[(x, b.list.y)].bg;
        assert_eq!(buf[(x, b.list.y + 1)].bg, bar, "{w}x{h}: the second line too");
        assert_ne!(buf[(x, b.list.y + 2)].bg, bar, "{w}x{h}: not the next result");
    }
}

#[test]
fn a_failure_is_its_own_sentence_says_the_wait_and_enter_meanwhile_says_what_is_left() {
    for lang in [Lang::En, Lang::Ko] {
        let mut s = signed(80, 24, lang);
        let i18n = I18n::new(lang);
        s.keys("/");
        type_words(&mut s, "deploy");
        s.keys("enter");
        s.b.fail(search_index(&s), FailKind::RateLimited { retry_after: Some(Duration::from_secs(20)) });
        s.pump();
        let screen = popup_text(&s);
        let why = format!("{}.", i18n.label(Label::FailRateLimited));
        assert!(has(&screen, &why), "{screen}");
        let then = i18n.msg(&Msg::SearchRetryIn { seconds: "20".into() }).to_string();
        assert!(has(&screen, &then), "{screen}");
        // Enter during the wait: nothing sent, what is left said.
        s.d.now += Duration::from_secs(8);
        s.keys("enter");
        assert!(searches(&s).is_empty(), "nothing sent during the wait");
        let left = i18n.msg(&Msg::SearchWaitLeft { seconds: "12".into() }).to_string();
        assert!(has(&popup_text(&s), &left), "{}", popup_text(&s));
        // Once it is over, Enter tries again.
        s.d.now += Duration::from_secs(12);
        s.keys("enter");
        assert_eq!(searches(&s), [("deploy".to_string(), 1)]);
    }
}

#[test]
fn the_status_line_names_the_workspace_of_a_result_opened_from_another() {
    let mut s = signed(120, 40, Lang::En);
    let theirs = World::demo().snapshot().workspaces[1].name.unsanitized().to_string();
    let target = conversation(1, "general");
    s.keys("/");
    type_words(&mut s, "x");
    s.keys("enter");
    s.b.held.clear();
    s.keys("tab");
    let m = World::demo().message(target.conversation(), 0).unwrap().ts;
    answer_with(&mut s, vec![hit(&target, m, None, "x")], 1, 1, 1);
    s.keys("enter");
    while !s.b.held.is_empty() {
        s.b.answer(0);
        s.pump();
    }
    let line = s.d.status_line();
    let short: String = theirs.chars().take(4).collect();
    assert!(line.contains(&short), "{line}");
}

#[test]
fn jump_refuses_a_day_to_come_and_says_when_nothing_was_written_on_or_after_the_day() {
    let mut s = signed(120, 40, Lang::En);
    let big = open_big(&mut s);
    s.keys(":");
    type_words(&mut s, "jump 2099-01-01");
    s.keys("enter");
    assert_eq!(s.d.app.overlay(), Some(Overlay::Palette));
    assert_eq!(s.d.app.cmdline.error, Some(Msg::JumpDateFuture { day: "2099-01-01".into() }));
    assert!(s.b.held.is_empty());
    s.keys("esc");
    // Two days after the newest message: read, the newest selected, and said.
    s.d.app.clock = Some(later);
    let w = World::demo();
    let newest = w.message(big.conversation(), w.message_count(big.conversation()) - 1).unwrap().ts;
    let day = time::date(Ts(newest.0 + 2 * DAY), &Zone::utc());
    s.keys(":");
    type_words(&mut s, &format!("jump {day}"));
    s.keys("enter");
    let (i, _) = dated(&s).unwrap_or_else(|| panic!("{:?} {:?}", s.d.app.cmdline.error, s.b.held));
    s.b.answer(i);
    s.pump();
    let p = s.d.app.focused_pane().unwrap();
    assert_eq!(p.selected().map(|i| p.messages()[i].ts), Some(newest));
    assert_eq!(notice(&s), Some(Msg::JumpDateQuiet { day }));
}

#[test]
fn jump_says_what_to_type_takes_the_language_s_words_and_closes_the_popup_it_was_run_over() {
    let mut s = signed(80, 24, Lang::Ko);
    let big = open_big(&mut s);
    s.keys(":");
    type_words(&mut s, "jump ");
    s.keys("enter");
    assert_eq!(s.d.app.cmdline.error, Some(Msg::Label(Label::JumpDateEmpty)));
    s.keys("esc");
    // Yesterday, in Korean.
    s.keys(":");
    type_words(&mut s, "jump \u{C5B4}\u{C81C}");
    s.keys("enter");
    let today = s.d.app.today();
    let (i, at) = dated(&s).expect("a day read");
    assert_eq!(at, slakio_tui::when::start(today - 1, &Zone::utc()));
    s.b.answer(i);
    s.pump();
    // From the palette opened over the search popup: the popup closes, the pane has the keys.
    s.keys("/");
    s.keys("ctrl+p");
    let w = World::demo();
    let early = time::date(w.message(big.conversation(), 100).unwrap().ts, &Zone::utc());
    type_words(&mut s, &format!("jump {early}"));
    s.keys("enter");
    assert!(!s.d.app.searching(), "the popup closed");
    assert_eq!(s.d.app.overlay(), None);
    assert_eq!(s.d.app.focused_pane().map(|p| p.target().clone()), Some(big));
}

#[test]
fn the_palette_s_keys_column_separates_keys_with_a_dot() {
    let mut s = signed(120, 40, Lang::En);
    s.keys(":");
    type_words(&mut s, "search");
    assert!(s.d.screen().contains("/ · Space s"), "{}", s.d.screen());
}

#[test]
fn in_seoul_the_days_and_times_shown_and_jumped_to_are_korea_s() {
    let mut s = signed(80, 24, Lang::Ko);
    let seoul = Zone::named("Asia/Seoul").expect("the system knows Asia/Seoul");
    s.d.app.zone = seoul.clone();
    s.d.app.clock = Some(later);
    let big = conversation(0, names::BIG_HISTORY);
    let w = World::demo();
    // A message written after 15:00 UTC: already the next day in Seoul.
    let n = w.message_count(big.conversation());
    let m = (n / 4..n / 2)
        .filter_map(|i| w.message(big.conversation(), i))
        .find(|m| (m.ts.secs() % 86_400) >= 15 * 3600 + 600)
        .expect("one written in the evening, UTC");
    let local = time::date(m.ts, &seoul);
    assert_ne!(local, time::date(m.ts, &Zone::utc()), "the day differs");
    open_big(&mut s);
    s.keys(":");
    type_words(&mut s, &format!("jump {local}"));
    s.keys("enter");
    let (i, at) = dated(&s).unwrap_or_else(|| panic!("{:?} {:?}", s.d.app.cmdline.error, s.b.held));
    // The day starts at Seoul's midnight: 15:00 UTC the day before.
    assert_eq!(at, seoul.start(time::day(m.ts, &seoul)));
    assert_eq!(at.secs() % 86_400, 15 * 3600);
    s.b.answer(i);
    s.pump();
    let p = s.d.app.focused_pane().unwrap();
    let first = p.selected().map(|i| p.messages()[i].ts).unwrap();
    assert!(first >= at && time::date(first, &seoul) == local);
    let screen = s.d.screen();
    assert!(screen.contains(&local), "the date row in Seoul's days: {screen}");
    assert!(screen.contains(&time::hm(first, &seoul)), "its time in Seoul's hours: {screen}");
}
