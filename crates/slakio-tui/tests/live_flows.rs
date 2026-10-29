//! Live updates in the app: a message posted shows at once in the pane open on its conversation
//! (the selection stays on its message, a pane following the newest keeps following), an edit
//! replaces it, a deletion takes it away, a reply counts under its thread's message and shows in
//! the open thread; the list's counts follow (a muted channel still badges its mentions only, the
//! user's own messages count for nothing), and a conversation read elsewhere takes Slack's counts.
//! The status line says when live updates start, stop and come back; when they come back the open
//! conversations and threads are read again, one page each, and the list takes Slack's counts
//! again. At 120x40 and 80x24.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
mod scripted;

use scripted::Scripted;
use slakio_core::backend::{Command, Connection, Envelope, Event, Generation, Live};
use slakio_core::i18n::Label;
use slakio_core::model::{ConversationId, Message, Target, Ts, UserId, WorkspaceId};
use slakio_core::sanitize::Remote;
use slakio_tui::app::PaneKind;
use std::time::Duration;

fn push(s: &mut Scripted, live: Live) {
    s.b.push(Envelope { generation: Generation::PUSHED, event: Event::Live(Box::new(live)) });
    s.pump();
}

fn message(ts: u64, user: &UserId, text: &str) -> Message {
    Message {
        ts: Ts(ts),
        user: user.clone(),
        text: Remote::new(text),
        thread: None,
        reactions: Vec::new(),
        edited: false,
    }
}

struct At {
    ws: WorkspaceId,
    conversation: ConversationId,
    other: UserId,
    me: UserId,
}

/// Booted, with `name` open and its newest page loaded.
fn opened(name: &str, w: u16, h: u16) -> (Scripted, At) {
    let mut s = Scripted::booted(true, None);
    s.d.app.resize(w, h);
    s.d.list_cursor_on(name);
    s.keys("enter");
    s.b.answer(0);
    s.pump();
    let c = s.d.app.model.conversations().iter().find(|c| c.name == name).unwrap().clone();
    let me = s.d.app.model.me(&c.workspace).unwrap().clone();
    let snapshot = slakio_world::World::demo().snapshot().clone();
    let other = snapshot.users.iter().find(|u| u.workspace == c.workspace && u.id != me && !u.bot).unwrap().id.clone();
    (s, At { ws: c.workspace, conversation: c.id, other, me })
}

impl At {
    fn post(&self, ts: u64, user: &UserId, text: &str) -> Live {
        self.live(None, false, false, message(ts, user, text))
    }

    fn live(&self, thread: Option<Ts>, changed: bool, mention: bool, message: Message) -> Live {
        Live::Message {
            workspace: self.ws.clone(),
            conversation: self.conversation.clone(),
            thread,
            broadcast: false,
            changed,
            mention,
            message,
            users: Vec::new(),
        }
    }
}

fn newest(s: &Scripted) -> Ts {
    s.d.pane(PaneKind::Conversation).unwrap().messages().last().unwrap().ts
}

#[test]
fn a_post_an_edit_and_a_deletion_show_at_once_at_120x40_and_80x24() {
    for (w, h) in [(120, 40), (80, 24)] {
        let (mut s, at) = opened("general", w, h);
        push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Connected { catch_up: false } });
        assert!(s.d.status_line().contains("Live updates on"), "{w}x{h}: {}", s.d.status_line());
        let ts = newest(&s).0 + 1_000_000;
        push(&mut s, at.post(ts, &at.other, "deploy is green"));
        assert_eq!(newest(&s), Ts(ts), "the pane follows the newest");
        assert!(s.d.screen().contains("deploy is green"), "{w}x{h}: {}", s.d.screen());
        insta::assert_snapshot!(format!("live_post_{w}x{h}"), s.d.snap());
        let mut edited = message(ts, &at.other, "deploy is red");
        edited.edited = true;
        push(&mut s, at.live(None, true, false, edited));
        let screen = s.d.screen();
        assert!(screen.contains("deploy is red") && !screen.contains("deploy is green"), "{w}x{h}: {screen}");
        push(
            &mut s,
            Live::Deleted { workspace: at.ws.clone(), conversation: at.conversation.clone(), thread: None, ts: Ts(ts) },
        );
        assert!(!s.d.screen().contains("deploy is red"), "{w}x{h}");
        assert_ne!(newest(&s), Ts(ts));
    }
}

#[test]
fn the_selection_stays_on_its_message_whatever_arrives() {
    let (mut s, at) = opened("general", 120, 40);
    s.keys("k k k");
    let pane = s.d.pane(PaneKind::Conversation).unwrap();
    let chosen = pane.messages()[pane.selected().expect("a selection")].ts;
    s.d.screen();
    let top = s.d.pane(PaneKind::Conversation).unwrap().drawn_rows();
    for i in 1..=5 {
        let ts = newest(&s).0 + i;
        push(&mut s, at.post(ts, &at.other, "more"));
    }
    let pane = s.d.pane(PaneKind::Conversation).unwrap();
    assert_eq!(pane.messages()[pane.selected().unwrap()].ts, chosen);
    s.d.screen();
    assert_eq!(s.d.pane(PaneKind::Conversation).unwrap().drawn_rows(), top, "nothing moved on screen");
}

#[test]
fn counts_follow_and_mute_still_rules() {
    let (mut s, at) = opened("general", 120, 40);
    let model = &s.d.app.model;
    let find = |m: &slakio_tui::app::model::Model, pred: &dyn Fn(&slakio_core::model::Conversation) -> bool| {
        m.conversations().iter().find(|c| c.workspace == at.ws && pred(c)).unwrap().clone()
    };
    let channel = find(model, &|c| !c.is_dm() && !c.muted && c.id != at.conversation);
    let muted = find(model, &|c| !c.is_dm() && c.muted);
    let dm = find(model, &|c| c.is_dm() && !c.muted);
    let post = |c: &slakio_core::model::Conversation, ts: u64, user: &UserId, mention: bool| Live::Message {
        workspace: at.ws.clone(),
        conversation: c.id.clone(),
        thread: None,
        broadcast: false,
        changed: false,
        mention,
        message: message(ts, user, "x"),
        users: Vec::new(),
    };
    let count = |s: &Scripted, c: &slakio_core::model::Conversation| {
        let c = s.d.app.model.conversations().iter().find(|x| x.id == c.id).unwrap();
        (c.unread, c.mentions, c.badges())
    };
    let before = count(&s, &channel);
    push(&mut s, post(&channel, 1, &at.other, false));
    push(&mut s, post(&channel, 2, &at.other, true));
    push(&mut s, post(&channel, 3, &at.me, true));
    let after = count(&s, &channel);
    assert_eq!((after.0, after.1), (before.0 + 2, before.1 + 1), "the user's own message counts for nothing");
    let before = count(&s, &muted);
    push(&mut s, post(&muted, 4, &at.other, false));
    push(&mut s, post(&muted, 5, &at.other, true));
    let after = count(&s, &muted);
    assert_eq!(after.2, (0, before.2.1 + 1), "a muted channel badges its mentions only");
    let before = count(&s, &dm);
    push(&mut s, post(&dm, 6, &at.other, false));
    assert_eq!(count(&s, &dm).1, before.1 + 1, "every message of a DM is for the user");
    // A reply counts in the conversation only when shown there too.
    let mut reply = post(&channel, 7, &at.other, true);
    if let Live::Message { thread, .. } = &mut reply {
        *thread = Some(Ts(1));
    }
    let before = count(&s, &channel);
    push(&mut s, reply);
    assert_eq!(count(&s, &channel), before);
    // Read elsewhere: Slack's counts.
    let marked = Live::Marked { workspace: at.ws.clone(), conversation: channel.id.clone(), unread: 0, mentions: 0 };
    push(&mut s, marked);
    assert_eq!((count(&s, &channel).0, count(&s, &channel).1), (0, 0));
}

#[test]
fn a_reply_counts_under_its_message_and_shows_in_the_open_thread() {
    let (mut s, at) = opened("general", 120, 40);
    s.keys("k");
    let pane = s.d.pane(PaneKind::Conversation).unwrap();
    let parent = pane.messages()[pane.selected().unwrap()].ts;
    let reply = |ts: u64| at.live(Some(parent), false, false, message(ts, &at.other, "on it"));
    let replies = |s: &Scripted| {
        let pane = s.d.pane(PaneKind::Conversation).unwrap();
        pane.messages().iter().find(|m| m.ts == parent).unwrap().thread.map(|t| (t.replies, t.last_reply))
    };
    let before = replies(&s).map_or(0, |r| r.0);
    let first = replies(&s).map_or(parent.0, |r| r.1.0) + 10;
    push(&mut s, reply(first));
    assert_eq!(replies(&s), Some((before + 1, Ts(first))));
    // The same reply twice counts once.
    push(&mut s, reply(first));
    assert_eq!(replies(&s).map(|r| r.0), Some(before + 1));
    assert!(!s.d.pane(PaneKind::Conversation).unwrap().messages().iter().any(|m| m.ts == Ts(first)));
    // With the thread open, the next one shows there too.
    let thread = Target::Thread { workspace: at.ws.clone(), conversation: at.conversation.clone(), thread: parent };
    s.keys("enter");
    let i =
        s.b.held
            .iter()
            .position(|r| matches!(&r.command, Command::History { target, .. } if *target == thread))
            .unwrap();
    s.b.answer(i);
    s.pump();
    let ts = first + 10;
    push(&mut s, reply(ts));
    let shown = s.d.app.pane_showing(&thread).unwrap().messages().iter().any(|m| m.ts == Ts(ts));
    assert!(shown, "in the thread");
    assert_eq!(replies(&s).map(|r| r.0), Some(before + 2));
    push(
        &mut s,
        Live::Deleted {
            workspace: at.ws.clone(),
            conversation: at.conversation.clone(),
            thread: Some(parent),
            ts: Ts(ts),
        },
    );
    assert_eq!(replies(&s).map(|r| r.0), Some(before + 1));
    assert!(!s.d.app.pane_showing(&thread).unwrap().messages().iter().any(|m| m.ts == Ts(ts)));
}

#[test]
fn lost_and_back_are_said_and_the_open_conversation_is_read_again_once() {
    let (mut s, at) = opened("general", 120, 40);
    let connected = |catch_up| Live::Connection { workspace: at.ws.clone(), state: Connection::Connected { catch_up } };
    push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Lost });
    assert!(s.d.status_line().contains("Live updates not connected: trying again"), "{}", s.d.status_line());
    push(&mut s, connected(false));
    push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Lost });
    assert!(s.d.status_line().contains("Live updates lost: reconnecting"), "{}", s.d.status_line());
    let target = Target::Conversation { workspace: at.ws.clone(), conversation: at.conversation.clone() };
    let live_ts = newest(&s).0 + 1;
    push(&mut s, at.post(live_ts, &at.other, "said while it was up"));
    push(&mut s, connected(true));
    assert!(s.d.status_line().contains("Live updates back"), "said in place of lost: {}", s.d.status_line());
    let catchups: Vec<_> = s.b.held.iter().filter(|r| matches!(r.command, Command::Catchup { .. })).collect();
    assert_eq!(catchups.len(), 1);
    assert!(matches!(&catchups[0].command, Command::Catchup { target: t, limit: 28 } if *t == target));
    // Back again before it answered: not asked twice.
    push(&mut s, connected(true));
    assert_eq!(s.b.held.iter().filter(|r| matches!(r.command, Command::Catchup { .. })).count(), 1);
    // Once it went, nothing stale comes after it.
    s.d.now += Duration::from_secs(7);
    s.d.app.on_tick(s.d.now);
    assert!(!s.d.status_line().contains("Live updates"), "{}", s.d.status_line());
    // The world's newest page is the truth: the message it does not have is gone.
    let i = s.b.held.iter().position(|r| matches!(r.command, Command::Catchup { .. })).unwrap();
    s.b.answer(i);
    s.pump();
    assert!(newest(&s) < Ts(live_ts));
    assert!(!s.d.screen().contains("said while it was up"));
}

#[test]
fn with_several_workspaces_the_notice_names_whose() {
    let mut s = Scripted::connected(true);
    let snapshot = slakio_world::World::demo().snapshot().clone();
    let signed: Vec<slakio_tui::app::model::SignedIn> = snapshot
        .workspaces
        .iter()
        .map(|w| slakio_tui::app::model::SignedIn {
            id: w.id.clone(),
            name: w.name.unsanitized().to_string(),
            read_only: true,
            user: String::new(),
        })
        .collect();
    s.d.app.connect_workspaces(s.b.caps, &signed);
    s.pump();
    while !s.b.held.is_empty() {
        s.b.answer(0);
        s.pump();
    }
    let b = &snapshot.workspaces[1];
    push(&mut s, Live::Connection { workspace: b.id.clone(), state: Connection::Connected { catch_up: false } });
    let name = b.name.unsanitized().to_string();
    assert!(s.d.status_line().contains(&format!("{name}: Live updates on")), "{}", s.d.status_line());
}

#[test]
fn what_comes_live_while_a_catch_up_is_on_its_way_stays_and_a_chosen_view_stays_put() {
    let (mut s, at) = opened("general", 120, 40);
    s.keys("k k k");
    let pane = s.d.pane(PaneKind::Conversation).unwrap();
    let chosen = pane.messages()[pane.selected().expect("a selection")].ts;
    s.d.screen();
    let top = s.d.pane(PaneKind::Conversation).unwrap().drawn_rows();
    push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Lost });
    push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Connected { catch_up: true } });
    // The socket is back, the page is on its way, and a message comes live meanwhile.
    let live_ts = 1_900_000_000_000_000;
    push(&mut s, at.post(live_ts, &at.other, "posted after the socket came back"));
    let i = s.b.held.iter().position(|r| matches!(r.command, Command::Catchup { .. })).unwrap();
    s.b.answer(i);
    s.pump();
    assert_eq!(newest(&s), Ts(live_ts), "kept after the page, which was read before it came");
    let pane = s.d.pane(PaneKind::Conversation).unwrap();
    assert_eq!(pane.messages()[pane.selected().unwrap()].ts, chosen);
    s.d.screen();
    assert_eq!(s.d.pane(PaneKind::Conversation).unwrap().drawn_rows(), top, "nothing moved on screen");
}

#[test]
fn a_failed_catch_up_is_asked_for_again_by_itself() {
    let (mut s, at) = opened("general", 120, 40);
    push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Lost });
    push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Connected { catch_up: true } });
    let catchups = |s: &Scripted| s.b.held.iter().filter(|r| matches!(r.command, Command::Catchup { .. })).count();
    let i = s.b.held.iter().position(|r| matches!(r.command, Command::Catchup { .. })).unwrap();
    s.b.fail(i, slakio_core::backend::FailKind::Network);
    s.pump();
    assert_eq!(catchups(&s), 0);
    s.d.now += Duration::from_secs(29);
    s.d.app.on_tick(s.d.now);
    s.pump();
    assert_eq!(catchups(&s), 0, "not before its pause");
    s.d.now += Duration::from_secs(2);
    s.d.app.on_tick(s.d.now);
    s.pump();
    assert_eq!(catchups(&s), 1, "asked again: no gap stays hidden");
}

#[test]
fn unknown_counts_stay_unknown_and_a_deletion_said_twice_counts_once() {
    let (mut s, at) = opened("general", 120, 40);
    let count = |s: &Scripted| {
        let c = s.d.app.model.conversations().iter().find(|c| c.id == at.conversation).unwrap();
        (c.unread, c.mentions)
    };
    s.d.app.model.counts_unknown.insert(at.ws.clone());
    let before = count(&s);
    let ts = newest(&s).0 + 1;
    push(&mut s, at.post(ts, &at.other, "x"));
    assert_eq!(count(&s), before, "one more on an unknown count is still unknown");
    s.d.app.model.counts_unknown.clear();
    // A reply: counted once, taken off once however often its deletion is said.
    s.keys("k");
    let pane = s.d.pane(PaneKind::Conversation).unwrap();
    let parent = pane.messages()[pane.selected().unwrap()].ts;
    let replies = |s: &Scripted| {
        let pane = s.d.pane(PaneKind::Conversation).unwrap();
        pane.messages().iter().find(|m| m.ts == parent).unwrap().thread.map_or(0, |t| t.replies)
    };
    let thread = Target::Thread { workspace: at.ws.clone(), conversation: at.conversation.clone(), thread: parent };
    s.keys("enter");
    let i =
        s.b.held
            .iter()
            .position(|r| matches!(&r.command, Command::History { target, .. } if *target == thread))
            .unwrap();
    s.b.answer(i);
    s.pump();
    let before = replies(&s);
    let ts = 1_900_000_000_000_000;
    push(&mut s, at.live(Some(parent), false, false, message(ts, &at.other, "on it")));
    assert_eq!(replies(&s), before + 1);
    let deleted = || Live::Deleted {
        workspace: at.ws.clone(),
        conversation: at.conversation.clone(),
        thread: Some(parent),
        ts: Ts(ts),
    };
    push(&mut s, deleted());
    push(&mut s, deleted());
    assert_eq!(replies(&s), before, "taken off once");
    // A reply never loaded: nothing to take off.
    push(
        &mut s,
        Live::Deleted {
            workspace: at.ws.clone(),
            conversation: at.conversation.clone(),
            thread: Some(parent),
            ts: Ts(7),
        },
    );
    assert_eq!(replies(&s), before);
}

/// Two workspaces signed in and started, each one's live updates on.
fn two_workspaces() -> (Scripted, Vec<slakio_core::model::Workspace>) {
    let mut s = Scripted::connected(true);
    let snapshot = slakio_world::World::demo().snapshot().clone();
    let signed: Vec<slakio_tui::app::model::SignedIn> = snapshot
        .workspaces
        .iter()
        .map(|w| slakio_tui::app::model::SignedIn {
            id: w.id.clone(),
            name: w.name.unsanitized().to_string(),
            read_only: true,
            user: String::new(),
        })
        .collect();
    s.d.app.connect_workspaces(s.b.caps, &signed);
    s.pump();
    while !s.b.held.is_empty() {
        s.b.answer(0);
        s.pump();
    }
    for w in &snapshot.workspaces {
        push(&mut s, Live::Connection { workspace: w.id.clone(), state: Connection::Connected { catch_up: false } });
    }
    s.d.now += Duration::from_secs(10);
    s.d.app.on_tick(s.d.now);
    (s, snapshot.workspaces)
}

#[test]
fn while_live_updates_are_down_the_status_line_keeps_saying_so_until_they_are_back() {
    for (w, h) in [(120, 40), (80, 24)] {
        let (mut s, at) = opened("general", w, h);
        assert!(!s.d.status_line().contains("live off"), "never heard of: nothing to say");
        push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Connected { catch_up: false } });
        assert!(!s.d.status_line().contains("live off"));
        push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Lost });
        let line = s.d.status_line();
        assert!(line.contains("Live updates lost: reconnecting") && line.contains("live off"), "{w}x{h}: {line}");
        // Long after the notice went, still said.
        for _ in 0..12 {
            s.d.now += Duration::from_secs(10);
            s.d.app.on_tick(s.d.now);
        }
        let line = s.d.status_line();
        assert!(line.contains("live off") && !line.contains("Live updates"), "{w}x{h}: {line}");
        push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Connected { catch_up: true } });
        let line = s.d.status_line();
        assert!(line.contains("Live updates back") && !line.contains("live off"), "{w}x{h}: {line}");
    }
}

#[test]
fn the_mark_fits_at_80_columns_in_korean_beside_the_notice() {
    let mut d = demo::Demo::with(80, 24, slakio_core::i18n::Lang::Ko, slakio_tui::app::Settings::default());
    let ws = d.app.model.workspaces()[0].id.clone();
    let lost = |state| Envelope {
        generation: Generation::PUSHED,
        event: Event::Live(Box::new(Live::Connection { workspace: ws.clone(), state })),
    };
    d.app.on_backend(lost(Connection::Connected { catch_up: false }), d.now);
    d.app.on_backend(lost(Connection::Lost), d.now);
    let ko = slakio_core::i18n::I18n::new(slakio_core::i18n::Lang::Ko);
    let said = |l| ko.label(l).to_string();
    let (off, lost_words, held) = (said(Label::StatusLiveOff), said(Label::LiveLost), said(Label::LiveHeld));
    let line = d.status_line();
    assert!(line.contains(&lost_words) && line.contains(&off), "{line}");
    d.app.on_backend(lost(Connection::Held), d.now);
    let line = d.status_line();
    assert!(line.contains(&held) && line.contains(&off), "{line}");
}

#[test]
fn held_by_the_budget_says_why_and_the_mark_too_where_the_line_is_wide() {
    let (mut s, at) = opened("general", 120, 40);
    push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Connected { catch_up: false } });
    push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Held });
    let line = s.d.status_line();
    assert!(line.contains("Live updates paused: request share used"), "{line}");
    assert!(line.contains("live off: share used"), "{line}");
    s.d.app.resize(80, 24);
    let line = s.d.status_line();
    assert!(line.contains("live off") && !line.contains("live off: share used"), "{line}");
    assert!(line.contains("Live updates paused: request share used"), "in full at 80: {line}");
}

#[test]
fn two_workspaces_lost_at_once_are_each_said_and_none_is_back_unsaid() {
    let (mut s, ws) = two_workspaces();
    let (a, b) = (&ws[0], &ws[1]);
    let (an, bn) = (a.name.unsanitized().to_string(), b.name.unsanitized().to_string());
    push(&mut s, Live::Connection { workspace: a.id.clone(), state: Connection::Lost });
    push(&mut s, Live::Connection { workspace: b.id.clone(), state: Connection::Lost });
    assert!(s.d.status_line().contains(&format!("{bn}: Live updates lost")), "{}", s.d.status_line());
    assert!(s.d.status_line().contains("live off"), "{}", s.d.status_line());
    s.d.now += Duration::from_secs(7);
    s.d.app.on_tick(s.d.now);
    assert!(s.d.status_line().contains(&format!("{an}: Live updates lost")), "A's turn: {}", s.d.status_line());
    push(&mut s, Live::Connection { workspace: a.id.clone(), state: Connection::Connected { catch_up: true } });
    assert!(s.d.status_line().contains(&format!("{an}: Live updates back")), "{}", s.d.status_line());
    assert!(s.d.status_line().contains("live off"), "B is still down: {}", s.d.status_line());
    push(&mut s, Live::Connection { workspace: b.id.clone(), state: Connection::Connected { catch_up: true } });
    for _ in 0..4 {
        s.d.now += Duration::from_secs(4);
        s.d.app.on_tick(s.d.now);
    }
    assert!(!s.d.status_line().contains("live off"), "{}", s.d.status_line());
    // A back again before its loss was seen (B's showed meanwhile): neither is said late.
    push(&mut s, Live::Connection { workspace: a.id.clone(), state: Connection::Lost });
    push(&mut s, Live::Connection { workspace: b.id.clone(), state: Connection::Lost });
    push(&mut s, Live::Connection { workspace: a.id.clone(), state: Connection::Connected { catch_up: true } });
    let mut said = Vec::new();
    for _ in 0..8 {
        said.push(s.d.status_line());
        s.d.now += Duration::from_secs(2);
        s.d.app.on_tick(s.d.now);
    }
    assert!(!said.iter().any(|l| l.contains(&format!("{an}: Live updates"))), "{said:#?}");
}

/// The screen row of the selected message's first row in the conversation pane.
fn selected_row(s: &mut Scripted) -> u16 {
    s.d.screen();
    let pane = s.d.pane(PaneKind::Conversation).unwrap();
    let i = pane.selected().unwrap();
    pane.drawn_rows().iter().filter(|(m, _)| *m == i).map(|(_, y)| *y).min().expect("on screen")
}

#[test]
fn a_catch_up_page_spliced_below_the_selection_leaves_it_on_its_row() {
    let (mut s, at) = opened("general", 120, 40);
    s.keys("k k k");
    let pane = s.d.pane(PaneKind::Conversation).unwrap();
    let chosen = pane.messages()[pane.selected().unwrap()].ts;
    let next = pane.messages()[pane.selected().unwrap() + 1].ts;
    let row = selected_row(&mut s);
    push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Lost });
    push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Connected { catch_up: true } });
    let i = s.b.held.iter().position(|r| matches!(r.command, Command::Catchup { .. })).unwrap();
    let mut answer = s.b.reply(i);
    let Event::History(page) = &mut answer.event else { panic!("{:?}", answer.event) };
    // Missed messages older than one loaded: they go in between the selection and the bottom.
    let at_ = page.messages.iter().position(|m| m.ts == next).unwrap();
    for k in (1..=12).rev() {
        page.messages.insert(at_, message(chosen.0 + k, &at.other, "missed meanwhile"));
    }
    s.b.push(answer);
    s.pump();
    assert!(s.d.screen().contains("missed meanwhile") || s.d.screen().contains("↓"), "{}", s.d.screen());
    let pane = s.d.pane(PaneKind::Conversation).unwrap();
    assert_eq!(pane.messages()[pane.selected().unwrap()].ts, chosen);
    assert_eq!(selected_row(&mut s), row, "the selection stays on its row");
    // What the splice put below the view counts as new there; what was shown before does not.
    let screen = s.d.screen();
    let shown = screen.matches("missed meanwhile").count();
    assert!(shown < 12, "some below the view: {screen}");
    assert_eq!(new_below(&screen), Some(12 - shown), "{screen}");
}

/// The `N` of `↓ N new` on the screen.
fn new_below(screen: &str) -> Option<usize> {
    let at = screen.find("↓ ")? + "↓ ".len();
    screen[at..].split_whitespace().next()?.parse().ok()
}

#[test]
fn coming_back_takes_slacks_counts_again_and_unknown_counts_are_known_again() {
    let (mut s, at) = opened("general", 120, 40);
    let counts = |s: &Scripted| -> Vec<(ConversationId, u32, u32)> {
        let all = s.d.app.model.conversations();
        all.iter().filter(|c| c.workspace == at.ws).map(|c| (c.id.clone(), c.unread, c.mentions)).collect()
    };
    let before = counts(&s);
    assert!(before.len() > 2);
    s.d.app.model.counts_unknown.insert(at.ws.clone());
    let random = before.iter().find(|(id, ..)| *id != at.conversation).unwrap().0.clone();
    let listed = vec![(at.conversation.clone(), 5, 2), (random.clone(), 1, 0)];
    push(&mut s, Live::Counts { workspace: at.ws.clone(), counts: listed });
    assert!(!s.d.app.model.counts_unknown.contains(&at.ws), "known again");
    for (id, unread, mentions) in counts(&s) {
        let want = if id == at.conversation {
            (5, 2)
        } else if id == random {
            (1, 0)
        } else {
            (0, 0)
        };
        assert_eq!((unread, mentions), want, "{id:?}: one Slack did not list has none");
    }
    // Another workspace's counts are left as they are.
    let elsewhere = WorkspaceId::new("TNOTHERE");
    push(&mut s, Live::Counts { workspace: elsewhere, counts: Vec::new() });
    assert_eq!(counts(&s).iter().find(|c| c.0 == at.conversation).map(|c| (c.1, c.2)), Some((5, 2)));
}

#[test]
fn coming_back_reads_the_open_thread_again_too() {
    let (mut s, at) = opened("general", 120, 40);
    s.keys("k");
    let pane = s.d.pane(PaneKind::Conversation).unwrap();
    let parent = pane.messages()[pane.selected().unwrap()].ts;
    let thread = Target::Thread { workspace: at.ws.clone(), conversation: at.conversation.clone(), thread: parent };
    s.keys("enter");
    let i =
        s.b.held
            .iter()
            .position(|r| matches!(&r.command, Command::History { target, .. } if *target == thread))
            .unwrap();
    s.b.answer(i);
    s.pump();
    push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Lost });
    push(&mut s, Live::Connection { workspace: at.ws.clone(), state: Connection::Connected { catch_up: true } });
    let asked: Vec<Target> =
        s.b.held
            .iter()
            .filter_map(|r| match &r.command {
                Command::Catchup { target, limit: 28 } => Some(target.clone()),
                _ => None,
            })
            .collect();
    let conversation = Target::Conversation { workspace: at.ws.clone(), conversation: at.conversation.clone() };
    assert_eq!(asked, [conversation, thread.clone()], "the conversation first, then its thread");
    // The reply missed meanwhile shows in the thread.
    let i = s.b.held.iter().position(|r| matches!(&r.command, Command::Catchup { target, .. } if *target == thread));
    let mut answer = s.b.reply(i.unwrap());
    let Event::History(page) = &mut answer.event else { panic!("{:?}", answer.event) };
    let last = page.messages.last().unwrap().ts;
    page.messages.push(message(last.0 + 1, &at.other, "missed reply"));
    s.b.push(answer);
    s.pump();
    let shown = s.d.app.pane_showing(&thread).unwrap().messages().iter().any(|m| m.ts == Ts(last.0 + 1));
    assert!(shown, "in the thread");
}

#[test]
fn a_held_view_says_how_many_came_below_it_until_the_newest_is_reached() {
    let (mut s, at) = opened("general", 120, 40);
    s.keys("k k k");
    assert!(!s.d.screen().contains("↓"));
    for i in 1..=3 {
        let ts = newest(&s).0 + i;
        push(&mut s, at.post(ts, &at.other, "more"));
    }
    assert!(s.d.screen().contains("↓ 3 new"), "{}", s.d.screen());
    s.keys("j");
    assert!(s.d.screen().contains("↓ 3 new"), "still below: {}", s.d.screen());
    s.keys("G");
    assert!(!s.d.screen().contains("↓"), "{}", s.d.screen());
    // The selection still holds the view: the next one is counted again, until the pane
    // follows the newest (`Esc`).
    let ts = newest(&s).0 + 1;
    push(&mut s, at.post(ts, &at.other, "after"));
    assert!(s.d.screen().contains("↓ 1 new"), "{}", s.d.screen());
    s.keys("esc");
    assert!(!s.d.screen().contains("↓"), "{}", s.d.screen());
    let ts = newest(&s).0 + 1;
    push(&mut s, at.post(ts, &at.other, "followed"));
    assert!(!s.d.screen().contains("↓") && s.d.screen().contains("followed"), "{}", s.d.screen());
}
