//! Slack's sidebar sections arriving after the boot answer: they replace the workspace's own,
//! each conversation moves to its section, and the list's cursor stays on its conversation even
//! when the header above it is gone. A DM closed in Slack's sidebar is not listed in Direct
//! messages, yet the DMs view lists it, a search result or a link opens it, and a message opens
//! it again.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
mod scripted;

use ratatui::crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyModifiers};
use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, Found, Generation, Hit, Live};
use slakio_core::model::{Conversation, ConversationId, Message, Section, SectionId, SectionKind, Target, Ts, UserId};
use slakio_core::sanitize::Remote;
use slakio_tui::app::model::{Row, SignedIn};
use slakio_tui::app::shell::View;
use slakio_world::World;

fn push(s: &mut Scripted, live: Live) {
    s.b.push(Envelope { generation: Generation::PUSHED, event: Event::Live(Box::new(live)) });
    s.pump();
}

/// The conversation under the list's cursor.
fn under_cursor(s: &Scripted) -> Option<String> {
    match s.d.app.list_rows().get(s.d.app.list_cursor())? {
        Row::Conversation(i) => Some(s.d.app.model.conversation(*i).name.unsanitized().to_string()),
        _ => None,
    }
}

#[test]
fn sections_that_come_later_replace_the_workspace_s_and_the_cursor_stays_on_its_conversation() {
    let mut s = Scripted::booted(true, None);
    s.d.list_cursor_on("incidents");
    let starred = s.d.app.model.conversations().iter().find(|c| c.name == "incidents").unwrap().clone();
    let ws = starred.workspace.clone();
    // A DM below the sections that go: its row moves up.
    let dm = (s.d.app.model.conversations().iter())
        .find(|c| c.workspace == ws && c.is_dm())
        .unwrap()
        .name
        .unsanitized()
        .to_string();
    s.d.list_cursor_on(&dm);
    let row_before = s.d.app.list_cursor();
    // Slack lists no Starred now: Channels and Direct messages only, everything in Channels
    // or Direct messages.
    let channels = Section {
        id: SectionId::new(format!("{}:channels", ws.as_str())),
        workspace: ws.clone(),
        name: "Channels".into(),
        kind: SectionKind::Channels,
    };
    let dms = Section {
        id: SectionId::new(format!("{}:dms", ws.as_str())),
        workspace: ws.clone(),
        name: "Direct messages".into(),
        kind: SectionKind::DirectMessages,
    };
    let places: Vec<(ConversationId, SectionId)> = (s.d.app.model.conversations().iter())
        .filter(|c| c.workspace == ws)
        .map(|c| (c.id.clone(), if c.is_dm() { dms.id.clone() } else { channels.id.clone() }))
        .collect();
    let rows_before = s.d.app.list_rows().len();
    push(&mut s, Live::Sections { workspace: ws.clone(), sections: vec![channels.clone(), dms], places });
    let kinds: Vec<SectionKind> = (s.d.app.list_rows().iter())
        .filter_map(|r| match r {
            Row::Section(x) => Some(s.d.app.model.section(*x).kind),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, [SectionKind::Channels, SectionKind::DirectMessages], "only Slack's sections");
    let moved = s.d.app.model.conversations().iter().find(|c| c.id == starred.id).unwrap();
    assert_eq!(moved.section, channels.id);
    assert_ne!(s.d.app.list_rows().len(), rows_before);
    assert_ne!(s.d.app.list_cursor(), row_before, "its row moved");
    assert_eq!(under_cursor(&s).as_deref(), Some(dm.as_str()), "the cursor stays on its conversation");
}

#[test]
fn sections_of_a_workspace_not_listed_change_nothing() {
    let mut s = Scripted::connected(true);
    s.pump();
    s.b.fail(0, slakio_core::backend::FailKind::Network);
    s.pump();
    let ws = slakio_core::model::WorkspaceId::new("T0NOTLISTED");
    let section = Section {
        id: SectionId::new("T0NOTLISTED:channels"),
        workspace: ws.clone(),
        name: "Channels".into(),
        kind: SectionKind::Channels,
    };
    push(&mut s, Live::Sections { workspace: ws, sections: vec![section], places: Vec::new() });
    s.keys("j");
    assert!(s.d.app.model.workspaces().is_empty());
    assert!(s.d.app.list_rows().is_empty());
}

/// Booted from the demo world with its first workspace's first DM closed, both workspaces
/// signed in on their hosts; that DM.
fn with_a_closed_dm() -> (Scripted, Conversation) {
    let mut snapshot = World::demo().snapshot().clone();
    let ws = snapshot.workspaces[0].id.clone();
    let dm = snapshot.conversations.iter_mut().find(|c| c.workspace == ws && c.is_dm()).unwrap();
    dm.closed = true;
    let dm = dm.clone();
    let mut s = Scripted::connected(true);
    s.b.snapshot = Some(snapshot.clone());
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
    (s, dm)
}

/// Whether the list panel lists conversation `id`.
fn lists(s: &Scripted, id: &ConversationId) -> bool {
    (s.d.app.list_rows().iter()).any(|r| matches!(r, Row::Conversation(i) if s.d.app.model.conversation(*i).id == *id))
}

fn target(c: &Conversation) -> Target {
    Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() }
}

#[test]
fn a_closed_dm_is_not_in_direct_messages_but_the_dms_view_lists_it() {
    let (mut s, dm) = with_a_closed_dm();
    s.d.app.show_unfolded(View::Home);
    assert!(!lists(&s, &dm.id), "closed: not in Direct messages");
    assert!(s.d.app.model.conversations().iter().any(|c| c.id == dm.id), "still known");
    s.d.app.show_unfolded(View::Dms);
    assert!(lists(&s, &dm.id), "the DMs view lists every DM");
}

#[test]
fn a_search_result_in_a_closed_dm_opens_it() {
    let (mut s, dm) = with_a_closed_dm();
    s.keys("space s");
    for c in "hello".chars() {
        s.d.app.handle_event(TermEvent::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)), s.d.now);
    }
    s.pump();
    s.keys("enter");
    let at = s.b.held.iter().position(|r| matches!(r.command, Command::Search { .. })).expect("a search asked");
    let r = s.b.held.remove(at);
    let Command::Search { query, .. } = r.command else { unreachable!() };
    let ts = Ts(1_767_603_600_000_100);
    let hit = Hit {
        conversation: dm.id.clone(),
        name: Remote::new("dm"),
        ts,
        thread: None,
        user: UserId::new("U-NOBODY"),
        author: Remote::new("someone"),
        text: Remote::new("hello"),
        permalink: String::new(),
    };
    let found = Found {
        workspace: r.workspace.unwrap(),
        query,
        page: 1,
        pages: 1,
        total: 1,
        hits: vec![hit],
        users: Vec::new(),
    };
    s.b.push(Envelope { generation: r.id, event: Event::Found(Box::new(found)) });
    s.pump();
    s.keys("enter");
    let want = target(&dm);
    assert!(
        s.b.held
            .iter()
            .any(|r| matches!(&r.command, Command::Around { target, at, .. } if *target == want && *at == ts)),
        "read around the result in the closed DM: {:?}",
        s.b.held
    );
}

#[test]
fn a_link_to_a_closed_dm_opens_it() {
    let (mut s, dm) = with_a_closed_dm();
    s.keys(":");
    s.d.type_text(&format!("open https://acme.slack.com/archives/{}", dm.id));
    s.keys("enter");
    let want = target(&dm);
    assert!(s.d.app.pane_showing(&want).is_some(), "the closed DM opens: {:?}", s.b.held);
}

#[test]
fn a_message_in_a_closed_dm_opens_it_again() {
    let (mut s, dm) = with_a_closed_dm();
    s.d.app.show_unfolded(View::Home);
    let me = s.d.app.model.me(&dm.workspace).unwrap().clone();
    let other =
        (World::demo().snapshot().users.iter()).find(|u| u.workspace == dm.workspace && u.id != me).unwrap().id.clone();
    let message = Message {
        ts: Ts(1_767_603_600_000_100),
        user: other,
        text: Remote::new("back"),
        thread: None,
        reactions: Vec::new(),
        edited: false,
        files: Vec::new(),
    };
    let live = Live::Message {
        workspace: dm.workspace.clone(),
        conversation: dm.id.clone(),
        thread: None,
        broadcast: false,
        changed: false,
        mention: false,
        message,
        users: Vec::new(),
    };
    push(&mut s, live);
    assert!(lists(&s, &dm.id), "open again, in Direct messages");
}

#[test]
fn a_built_in_section_without_a_name_is_named_in_the_ui_s_language_and_one_of_the_user_s_own_keeps_its_name() {
    use slakio_core::i18n::{I18n, Label, Lang};
    for lang in [Lang::En, Lang::Ko] {
        let i18n = I18n::new(lang);
        let label = |l: Label| i18n.label(l).to_string();
        let names = [
            label(Label::SectionStarred),
            "Mine".to_string(),
            label(Label::SectionExternal),
            label(Label::SectionChannels),
            label(Label::SectionDms),
            label(Label::SectionApps),
        ];
        if lang == Lang::En {
            assert_eq!(names[0], "Starred", "the English names are these");
            assert_eq!(names[2..], ["External connections", "Channels", "Direct messages", "Apps"]);
        } else {
            assert!(names.iter().all(|n| !n.is_ascii() || n == "Mine"), "the Korean names are the locale's");
        }
        let mut snapshot = World::demo().snapshot().clone();
        let ws = snapshot.workspaces[0].id.clone();
        let kinds = [
            SectionKind::Favorites,
            SectionKind::Custom,
            SectionKind::External,
            SectionKind::Channels,
            SectionKind::DirectMessages,
            SectionKind::Apps,
        ];
        let sections: Vec<Section> = (kinds.iter().enumerate())
            .map(|(i, kind)| Section {
                id: SectionId::new(format!("{ws}:s{i}")),
                workspace: ws.clone(),
                name: if *kind == SectionKind::Custom { "Mine".into() } else { "".into() },
                kind: *kind,
            })
            .collect();
        // One conversation in each (the others of the workspace left out), so every header
        // shows on the screen.
        let mut ids = sections.iter().map(|x| x.id.clone());
        snapshot.conversations.retain_mut(|c| {
            if c.workspace != ws {
                return true;
            }
            ids.next().map(|id| c.section = id).is_some()
        });
        snapshot.sections.retain(|x| x.workspace != ws);
        snapshot.sections.splice(0..0, sections);
        let mut s = Scripted::connected(true);
        s.d.app.i18n = I18n::new(lang);
        s.d.app.resize(120, 80);
        s.b.snapshot = Some(snapshot);
        s.pump();
        s.b.answer(0);
        s.pump();
        s.d.app.show_unfolded(View::Home);
        // A wide character is drawn with a blank after it, and a long name is clipped.
        let screen = s.d.screen().replace(' ', "");
        for name in &names {
            let start: String = name.replace(' ', "").chars().take(8).collect();
            assert!(screen.contains(&start), "{lang:?}: {name} in {screen}");
        }
    }
}
