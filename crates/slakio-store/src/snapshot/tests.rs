use super::*;
use crate::testutil::{page, target, writer};
use slakio_world::World;

fn ids(s: &Snapshot) -> Vec<WorkspaceId> {
    s.workspaces.iter().map(|w| w.id.clone()).collect()
}

/// Each workspace's part comes back as that workspace's part of `s`.
fn assert_parts(store: &Store, s: &Snapshot, what: &str) {
    for ws in ids(s) {
        assert_eq!(store.snapshot(std::slice::from_ref(&ws)).unwrap(), Some(s.only(&ws)), "{what}: {ws}");
    }
}

#[test]
fn a_snapshot_comes_back_exactly_as_it_was_kept() {
    for (tag, world) in [("demo", World::demo()), ("seed7", World::new(7)), ("seed11", World::new(11))] {
        let mut store = writer(&format!("snap-{tag}"));
        let s = world.snapshot();
        assert!(s.users.iter().any(|u| matches!(u.org, Org::External(_))), "{tag}: the world has a Slack Connect peer");
        store.put_snapshot(s).unwrap();
        assert_parts(&store, s, tag);
        let both = store.snapshot(&ids(s)).unwrap().unwrap();
        assert_eq!(both.workspaces, s.workspaces, "{tag}: both, in the order asked");
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
    let mut with_other = ids(s);
    with_other.push(WorkspaceId::new("TOTHER"));
    assert_eq!(store.snapshot(&with_other).unwrap(), None, "all asked for, or nothing");
    assert_eq!(store.snapshot(&[]).unwrap(), None);
    assert_eq!(store.snapshot(&ids(s)[..1]).unwrap(), Some(s.only(&ids(s)[0])), "one workspace's part alone");
}

#[test]
fn a_workspaces_new_snapshot_replaces_its_own_part_and_never_another_s() {
    let world = World::new(5);
    let mut store = writer("snap-replace");
    store.put_snapshot(world.snapshot()).unwrap();
    let (first, second) = (ids(world.snapshot())[0].clone(), ids(world.snapshot())[1].clone());
    let t = target(&world, "general");
    store.put_page(&page(&world, &t, None, 10), None, 100).unwrap();
    // The second workspace's boot again, with a conversation fewer.
    let mut b = world.snapshot().only(&second);
    b.conversations.pop();
    store.put_snapshot(&b).unwrap();
    assert_eq!(store.snapshot(std::slice::from_ref(&second)).unwrap().as_ref(), Some(&b));
    assert_eq!(store.snapshot(std::slice::from_ref(&first)).unwrap(), Some(world.snapshot().only(&first)));
    assert!(store.newest_page(&t, 10).unwrap().is_some(), "the other workspace keeps its messages");
    // Nothing of a workspace its snapshot does not list is removed by it.
    let kept: i64 = store
        .conn
        .query_row("SELECT COUNT(*) FROM users WHERE workspace = ?1", [first.as_str()], |r| r.get(0))
        .unwrap();
    assert!(kept > 0);
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
    assert_parts(&store, s, "the snapshot's people as they were, in order");
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
