//! A record of a person that does not know whether they are around (a page of history's names,
//! a search's, Activity's: Slack tells no presence there) keeps the presence known of them: a
//! DM's `◐` stays after the DM is opened, a search, or Activity. A record that knows replaces it.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use scripted::Scripted;
use slakio_core::backend::{Command, Event};
use slakio_core::model::{ConversationKind, Org, Presence, User, UserId, WorkspaceId};
use slakio_core::sanitize::Remote;
use slakio_tui::app::model::{Model, Row};
use slakio_tui::app::shell::View;

fn person(ws: &WorkspaceId, presence: Presence) -> User {
    User {
        id: UserId::new("U-peer"),
        workspace: ws.clone(),
        name: Remote::new("peer"),
        display_name: Remote::new("Peer"),
        org: Org::Own,
        bot: false,
        presence,
        avatar: None,
        no_photo: false,
        avatar_guessed: false,
    }
}

#[test]
fn an_unknown_presence_keeps_the_one_known_and_a_known_one_replaces_it() {
    let ws = WorkspaceId::new("T1");
    let id = UserId::new("U-peer");
    let mut m = Model::default();
    let now = |m: &Model| m.user(&ws, &id).map(|u| u.presence);
    m.add_users(&ws, &[person(&ws, Presence::Unknown)]);
    assert_eq!(now(&m), Some(Presence::Unknown));
    m.add_users(&ws, &[person(&ws, Presence::Dnd)]);
    assert_eq!(now(&m), Some(Presence::Dnd), "a known presence replaces an unknown one");
    let renamed = User { display_name: Remote::new("Renamed"), ..person(&ws, Presence::Unknown) };
    m.add_users(&ws, &[renamed]);
    assert_eq!(now(&m), Some(Presence::Dnd), "an unknown one keeps it");
    assert_eq!(m.user(&ws, &id).unwrap().display_name.unsanitized(), "Renamed", "the rest is the new record's");
    m.add_users(&ws, &[person(&ws, Presence::Active)]);
    assert_eq!(now(&m), Some(Presence::Active), "a known presence replaces another known one");
    m.add_users(&ws, &[person(&ws, Presence::Away)]);
    assert_eq!(now(&m), Some(Presence::Away));
    m.change_users(&ws, &[person(&ws, Presence::Unknown)]);
    assert_eq!(now(&m), Some(Presence::Away), "a live profile change keeps it too");
}

/// Booted over a scripted backend, the list on DMs.
fn dms() -> Scripted {
    let mut s = Scripted::booted(true, None);
    s.keys("space d");
    assert_eq!(s.d.app.view(), View::Dms);
    s
}

/// The first DM listed whose peer is in do not disturb: its row and its peer.
fn dnd_peer(s: &Scripted) -> (usize, User) {
    let m = &s.d.app.model;
    let rows = s.d.app.list_rows();
    (0..rows.len())
        .find_map(|r| {
            let Row::Conversation(i) = rows[r] else { return None };
            let ConversationKind::Dm { user } = &m.conversation(i).kind else { return None };
            m.user(&m.conversation(i).workspace, user).filter(|u| u.presence == Presence::Dnd).map(|u| (r, u.clone()))
        })
        .expect("a DM peer in do not disturb")
}

/// The mark drawn on list row `row` (DMs has no sections: the cell after the gutter, or after
/// the avatar chip there).
fn mark(s: &Scripted, row: usize) -> String {
    let list = s.d.app.list_parts().expect("the list shown").rows;
    let y = list.y + (row - s.d.app.list_top()) as u16;
    let x = list.x + if s.d.app.settings.avatars { 3 } else { 1 };
    s.d.buffer()[(x, y)].symbol().to_string()
}

/// `peer` as a page of names gives them: whether they are around unknown.
fn unknown(peer: &User) -> User {
    User { presence: Presence::Unknown, ..peer.clone() }
}

fn presence(s: &Scripted, peer: &User) -> Presence {
    s.d.app.model.user(&peer.workspace, &peer.id).expect("the peer").presence
}

#[test]
fn opening_a_dm_keeps_its_peers_do_not_disturb() {
    let mut s = dms();
    let (row, peer) = dnd_peer(&s);
    assert_eq!(mark(&s, row), "◐");
    s.d.app.move_list_cursor_to(row);
    s.keys("enter");
    let i =
        (s.b.held.iter()).position(|r| matches!(r.command, Command::History { .. })).expect("the DM's history asked");
    let mut e = s.b.reply(i);
    let Event::History(page) = &mut e.event else { panic!("{e:?}") };
    page.users.retain(|u| u.id != peer.id);
    page.users.push(unknown(&peer));
    s.b.push(e);
    s.pump();
    assert_eq!(presence(&s, &peer), Presence::Dnd);
    s.keys("space d");
    assert_eq!(mark(&s, row), "◐", "the row still shows do not disturb");
}

#[test]
fn a_search_keeps_a_peers_do_not_disturb() {
    let mut s = dms();
    let (row, peer) = dnd_peer(&s);
    s.keys("/");
    for c in "deploy".chars() {
        s.keys(&c.to_string());
    }
    s.keys("enter");
    let i = (s.b.held.iter()).position(|r| matches!(r.command, Command::Search { .. })).expect("a search asked");
    let mut e = s.b.reply(i);
    let Event::Found(found) = &mut e.event else { panic!("{e:?}") };
    found.users.push(unknown(&peer));
    s.b.push(e);
    s.pump();
    assert!(s.d.app.search_found().is_some(), "the results shown");
    assert_eq!(presence(&s, &peer), Presence::Dnd);
    s.keys("esc");
    assert_eq!(mark(&s, row), "◐", "the row still shows do not disturb");
}

#[test]
fn activity_keeps_a_peers_do_not_disturb() {
    let mut s = dms();
    let (row, peer) = dnd_peer(&s);
    s.keys("space a");
    assert_eq!(s.d.app.view(), View::Activity);
    let mut answered = 0;
    while let Some(i) = s.b.held.iter().position(|r| matches!(r.command, Command::Activity { .. })) {
        let mut e = s.b.reply(i);
        if let Event::Activity(feed) = &mut e.event {
            feed.users.push(unknown(&peer));
            answered += 1;
        }
        s.b.push(e);
        s.pump();
    }
    assert!(answered > 0, "Activity asked");
    assert_eq!(presence(&s, &peer), Presence::Dnd);
    s.keys("space d");
    assert_eq!(mark(&s, row), "◐", "the row still shows do not disturb");
}
