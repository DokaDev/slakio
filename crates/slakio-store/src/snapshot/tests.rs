use super::*;
use crate::testutil::{page, target, writer};
use slakio_world::World;

fn ids(s: &Snapshot) -> Vec<WorkspaceId> {
    s.workspaces.iter().map(|w| w.id.clone()).collect()
}

#[test]
fn a_snapshot_comes_back_exactly_as_it_was_kept() {
    for (tag, world) in [("demo", World::demo()), ("seed7", World::new(7)), ("seed11", World::new(11))] {
        let mut store = writer(&format!("snap-{tag}"));
        let s = world.snapshot();
        assert!(s.users.iter().any(|u| matches!(u.org, Org::External(_))), "{tag}: the world has a Slack Connect peer");
        store.put_snapshot(s).unwrap();
        assert_eq!(store.snapshot(&ids(s)).unwrap().as_ref(), Some(s), "{tag}");
    }
}

#[test]
fn saved_items_that_were_unknown_stay_unknown_and_none_stays_empty() {
    let mut store = writer("snap-saved");
    let mut s = World::new(5).snapshot().clone();
    s.saved = None;
    store.put_snapshot(&s).unwrap();
    assert_eq!(store.snapshot(&ids(&s)).unwrap().unwrap().saved, None, "unknown is not empty");
    s.saved = Some(Vec::new());
    store.put_snapshot(&s).unwrap();
    assert_eq!(store.snapshot(&ids(&s)).unwrap().unwrap().saved, Some(Vec::new()));
}

#[test]
fn remote_text_is_kept_as_it_came_for_the_sanitiser_to_read() {
    let mut store = writer("snap-hostile");
    let mut s = World::new(5).snapshot().clone();
    let hostile = "\u{1b}]52;c;AAAA\u{7}\u{202e}evil\u{0}";
    s.conversations[0].name = Remote::new(hostile);
    s.users[0].display_name = Remote::new(hostile);
    store.put_snapshot(&s).unwrap();
    let back = store.snapshot(&ids(&s)).unwrap().unwrap();
    assert_eq!(back.conversations[0].name.unsanitized(), hostile);
    assert_eq!(back.users[0].display_name.unsanitized(), hostile);
    assert!(!back.conversations[0].name.line().as_str().contains('\u{1b}'), "it is Remote: sanitised when drawn");
}

#[test]
fn nothing_kept_or_another_sign_ins_snapshot_is_none() {
    let mut store = writer("snap-none");
    let world = World::new(5);
    let s = world.snapshot();
    assert_eq!(store.snapshot(&ids(s)).unwrap(), None);
    store.put_snapshot(s).unwrap();
    assert_eq!(store.snapshot(&[WorkspaceId::new("TOTHER")]).unwrap(), None);
    assert_eq!(store.snapshot(&ids(s)[..1]).unwrap(), None, "a part of it is not it");
}

#[test]
fn a_new_snapshot_replaces_the_last_and_drops_the_workspaces_it_no_longer_lists() {
    let world = World::new(5);
    let mut store = writer("snap-replace");
    store.put_snapshot(world.snapshot()).unwrap();
    let t = target(&world, "general");
    store.put_page(&page(&world, &t, None, 10), None, 100).unwrap();
    // Signed in to the first workspace alone now.
    let mut alone = world.snapshot().clone();
    let first = alone.workspaces[0].id.clone();
    alone.workspaces.truncate(1);
    alone.users.retain(|u| u.workspace == first);
    alone.sections.retain(|s| s.workspace == first);
    alone.conversations.retain(|c| c.workspace == first);
    store.put_snapshot(&alone).unwrap();
    assert_eq!(store.snapshot(std::slice::from_ref(&first)).unwrap().as_ref(), Some(&alone));
    assert!(store.newest_page(&t, 10).unwrap().is_some(), "the workspace still listed keeps its messages");
    let gone: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM users WHERE workspace != ?1", [first.as_str()], |r| r.get(0))
        .unwrap();
    assert_eq!(gone, 0, "nothing of the other workspace is left");
    // Signed in elsewhere: the first's messages go too.
    let mut elsewhere = alone.clone();
    elsewhere.workspaces[0].id = WorkspaceId::new("TELSEWHERE");
    store.put_snapshot(&elsewhere).unwrap();
    assert!(store.newest_page(&t, 10).unwrap().is_none());
}

#[test]
fn people_a_page_brought_are_not_in_the_snapshot_and_keep_its_order_otherwise() {
    let world = World::new(5);
    let mut store = writer("snap-users");
    let s = world.snapshot();
    store.put_snapshot(s).unwrap();
    let t = target(&world, "general");
    let mut p = page(&world, &t, None, 5);
    let mut stranger = s.users[0].clone();
    stranger.id = UserId::new("UPAGEONLY");
    p.users = vec![stranger, s.users[3].clone()];
    store.put_page(&p, None, 100).unwrap();
    assert_eq!(store.snapshot(&ids(s)).unwrap().as_ref(), Some(s), "the snapshot's people as they were, in order");
}

#[test]
fn a_value_this_version_cannot_read_is_an_error_not_a_guess() {
    let mut store = writer("snap-damaged");
    let world = World::new(5);
    let s = world.snapshot();
    store.put_snapshot(s).unwrap();
    store.conn.execute("UPDATE users SET presence = 42 WHERE ord = 0", []).unwrap();
    assert!(store.snapshot(&ids(s)).is_err());
    store.put_snapshot(s).unwrap();
    store.conn.execute("UPDATE conversations SET peers = 'x' WHERE ord = 0", []).unwrap();
    assert!(store.snapshot(&ids(s)).is_err());
}
