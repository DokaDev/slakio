use super::*;
use crate::testutil::{other, page, target, writer};
use slakio_world::World;

fn texts(p: &Page) -> Vec<String> {
    p.messages.iter().map(|m| m.text.unsanitized().to_string()).collect()
}

#[test]
fn the_newest_messages_come_back_as_they_were_kept_oldest_first() {
    let world = World::new(9);
    let mut store = writer("msg-roundtrip");
    let t = target(&world, "general");
    let mut p = page(&world, &t, None, 28);
    // Every field set somewhere: a thread, reactions (one with a separator in its name), an edit.
    p.messages[0].thread = Some(ThreadSummary { replies: 3, last_reply: Ts(p.messages[0].ts.0 + 5) });
    p.messages[1].reactions = vec![
        Reaction { name: Remote::new("+1"), count: 2, mine: true },
        Reaction { name: Remote::new("3:odd:name"), count: 1, mine: false },
    ];
    p.messages[2].edited = true;
    p.users = vec![world.snapshot().users[0].clone()];
    store.put_page(&p, None, 1_000).unwrap();
    let back = store.newest_page(&t, 28).unwrap().unwrap();
    assert_eq!(back.messages, p.messages);
    assert!(!back.complete, "the backend's own page says whether older ones exist");
    let fewer = store.newest_page(&t, 5).unwrap().unwrap();
    assert_eq!(fewer.messages, p.messages[p.messages.len() - 5..], "the newest five");
}

#[test]
fn a_page_names_the_people_of_its_messages_whom_the_cache_knows() {
    let world = World::new(9);
    let mut store = writer("msg-users");
    store.put_snapshot(world.snapshot()).unwrap();
    let t = target(&world, "general");
    let p = page(&world, &t, None, 28);
    store.put_page(&p, None, 1).unwrap();
    let back = store.newest_page(&t, 28).unwrap().unwrap();
    assert!(!back.users.is_empty());
    for u in &back.users {
        assert!(back.messages.iter().any(|m| m.user == u.id), "{:?} wrote none of them", u.id);
    }
}

#[test]
fn a_page_is_the_truth_for_its_span_deleted_messages_go_edits_replace() {
    let world = World::new(9);
    let mut store = writer("msg-span");
    let t = target(&world, "general");
    let newest = page(&world, &t, None, 10);
    let older = page(&world, &t, Some(newest.messages[0].ts), 10);
    store.put_page(&older, Some(newest.messages[0].ts), 1).unwrap();
    store.put_page(&newest, None, 1).unwrap();
    // Since then one message was deleted and one edited.
    let mut again = newest.clone();
    let deleted = again.messages.remove(4);
    again.messages[5].text = Remote::new("edited later");
    store.put_page(&again, None, 2).unwrap();
    let back = store.newest_page(&t, 100).unwrap().unwrap();
    assert!(!back.messages.iter().any(|m| m.ts == deleted.ts), "the deleted one is gone");
    assert!(texts(&back).contains(&"edited later".to_string()));
    assert_eq!(back.messages.len(), 19, "the older page, outside the span, is kept");
    assert_eq!(back.messages[..10], older.messages[..]);
}

#[test]
fn a_complete_page_clears_everything_older_and_an_empty_one_everything() {
    let world = World::new(9);
    let mut store = writer("msg-complete");
    let t = target(&world, "general");
    let p = page(&world, &t, None, 10);
    store.put_page(&p, None, 1).unwrap();
    let last = Page { messages: p.messages[8..].to_vec(), complete: true, ..p.clone() };
    store.put_page(&last, None, 2).unwrap();
    assert_eq!(store.newest_page(&t, 100).unwrap().unwrap().messages, last.messages);
    let empty = Page { messages: Vec::new(), complete: true, ..p };
    store.put_page(&empty, None, 3).unwrap();
    assert_eq!(store.newest_page(&t, 100).unwrap(), None, "nothing is there any more");
}

#[test]
fn threads_are_not_kept_and_conversations_do_not_mix() {
    let world = World::new(9);
    let mut store = writer("msg-threads");
    let t = target(&world, "general");
    let p = page(&world, &t, None, 10);
    let thread = Target::Thread {
        workspace: t.workspace().clone(),
        conversation: t.conversation().clone(),
        thread: p.messages[0].ts,
    };
    store.put_page(&Page { target: thread.clone(), ..p.clone() }, None, 1).unwrap();
    assert_eq!(store.newest_page(&thread, 10).unwrap(), None);
    assert_eq!(store.newest_page(&t, 10).unwrap(), None, "a thread's page is not its conversation's");
    store.put_page(&p, None, 1).unwrap();
    assert_eq!(store.newest_page(&other(t.workspace().as_str(), "COTHER"), 10).unwrap(), None);
    assert_eq!(store.newest_page(&other("TOTHER", t.conversation().as_str()), 10).unwrap(), None);
}

#[test]
fn a_damaged_reaction_list_is_an_error() {
    let world = World::new(9);
    let mut store = writer("msg-damaged");
    let t = target(&world, "general");
    store.put_page(&page(&world, &t, None, 3), None, 1).unwrap();
    store.conn.execute("UPDATE messages SET reactions = '1:a1:22:no'", []).unwrap();
    assert!(store.newest_page(&t, 3).is_err());
}

#[test]
fn a_read_only_cache_keeps_no_page() {
    let world = World::new(9);
    let dir = crate::testutil::scratch("msg-ro").join("cache");
    let _w = Store::open(&dir).unwrap().store;
    let mut reader = Store::open(&dir).unwrap().store;
    let t = target(&world, "general");
    reader.put_page(&page(&world, &t, None, 3), None, 1).unwrap();
    assert_eq!(reader.newest_page(&t, 3).unwrap(), None);
}
