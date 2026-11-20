use super::*;
use crate::testutil::{other, page, target, writer};
use slakio_core::model::WorkspaceId;
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
    // Files: one with every field (a separator in its name), one with none it may leave out.
    p.messages[3].files = vec![
        File {
            id: FileId::new("F1"),
            name: Remote::new("5:odd:name.pdf"),
            kind: Remote::new("PDF"),
            size: Some(1234),
            download: Some(format!("https://{}/files-pri/T-F1/download/x.pdf", slakio_core::model::FILE_HOST)),
            thumb: Some(format!("https://{}/files-tmb/T-F1/x_360.png", slakio_core::model::FILE_HOST)),
        },
        File {
            id: FileId::new("F2"),
            name: Remote::new(""),
            kind: Remote::new(""),
            size: None,
            download: None,
            thumb: None,
        },
    ];
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
fn a_thread_s_page_is_not_its_conversation_s_and_conversations_do_not_mix() {
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
    assert_eq!(
        store.newest_page(&thread, 10).unwrap().map(|b| b.messages),
        Some(p.messages.clone()),
        "kept as the thread's"
    );
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

#[test]
fn a_page_never_writes_the_people_of_another_workspace() {
    let world = World::new(9);
    let mut store = writer("msg-other-ws");
    let t = target(&world, "general");
    let mut p = page(&world, &t, None, 5);
    let mut stranger = world.snapshot().users[0].clone();
    stranger.workspace = WorkspaceId::new("T0OTHER");
    p.users = vec![world.snapshot().users[0].clone(), stranger];
    store.put_page(&p, None, 1).unwrap();
    let count = |ws: &str| -> i64 {
        store.conn.query_row("SELECT COUNT(*) FROM users WHERE workspace = ?1", [ws], |r| r.get(0)).unwrap()
    };
    assert_eq!(count(t.workspace().as_str()), 1, "the target's workspace's person is kept");
    assert_eq!(count("T0OTHER"), 0, "another workspace's part is not written");
}

/// The thread under message `thread` of `t`'s conversation.
fn thread_of(t: &Target, thread: Ts) -> Target {
    Target::Thread { workspace: t.workspace().clone(), conversation: t.conversation().clone(), thread }
}

#[test]
fn a_thread_is_kept_apart_from_its_conversation() {
    let world = World::new(9);
    let mut store = writer("msg-thread");
    let t = target(&world, "general");
    let conv = page(&world, &t, None, 28);
    store.put_page(&conv, None, 1).unwrap();
    let th = thread_of(&t, conv.messages[0].ts);
    let replies = Page {
        target: th.clone(),
        messages: conv.messages[..5].iter().map(|m| Message { text: Remote::new("reply"), ..m.clone() }).collect(),
        complete: true,
        users: Vec::new(),
        cut: false,
    };
    store.put_page(&replies, None, 2).unwrap();
    let back = store.newest_page(&th, 28).unwrap().unwrap();
    assert_eq!(back.messages, replies.messages);
    assert_eq!(back.target, th);
    assert_eq!(store.newest_page(&t, 28).unwrap().unwrap().messages, conv.messages, "the conversation's are its own");
    // A thread's page is the truth for its span too: a reply deleted since goes.
    let fewer = Page { messages: replies.messages[1..].to_vec(), ..replies.clone() };
    store.put_page(&fewer, None, 3).unwrap();
    assert_eq!(store.newest_page(&th, 28).unwrap().unwrap().messages.len(), 4);
    // Another thread of the conversation has nothing kept.
    assert_eq!(store.newest_page(&thread_of(&t, Ts(1)), 28).unwrap(), None);
}

#[test]
fn damaged_files_are_damage_not_none() {
    assert_eq!(unfiles("", "").unwrap(), Vec::new());
    assert!(unfiles("2:F1", "").is_err(), "too few parts");
    assert!(unfiles("2:F14:name3:PDF3:abc0:", "").is_err(), "a size that is not a number");
    assert!(unfiles("9:F", "").is_err(), "not packed");
}

#[test]
fn thumbnails_follow_their_files_and_damaged_ones_are_damage() {
    let one = "2:F14:name3:PNG1:20:";
    assert_eq!(unfiles(one, "").unwrap()[0].thumb, None, "kept before thumbnails were: none");
    assert_eq!(unfiles(one, "0:").unwrap()[0].thumb, None, "an empty one is none");
    assert!(unfiles(one, "0:0:").is_err(), "more thumbnails than files");
    assert!(unfiles(one, "9:x").is_err(), "not packed");
}

#[test]
fn a_thumbnail_kept_on_another_host_is_none_when_read() {
    let one = "2:F14:name3:PNG1:20:";
    let packed = pack(&["https://evil.example/x.png".to_string()]);
    assert_eq!(unfiles(one, &packed).unwrap()[0].thumb, None);
    let good = format!("https://{}/files-tmb/T-F1/x_360.png", slakio_core::model::FILE_HOST);
    assert_eq!(unfiles(one, &pack(std::slice::from_ref(&good))).unwrap()[0].thumb, Some(good));
}
