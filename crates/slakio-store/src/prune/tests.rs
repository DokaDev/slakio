use super::*;
use crate::testutil::{other, writer};
use slakio_core::backend::Page;
use slakio_core::model::{Message, Ts, UserId};
use slakio_core::sanitize::Remote;

const DAY: u64 = 86_400;

/// `n` messages of `text_len` bytes each, the newest at `ts` `n`.
fn messages(n: u64, text_len: usize) -> Vec<Message> {
    (1..=n)
        .map(|i| Message {
            ts: Ts(i * 1_000_000),
            user: UserId::new("U1"),
            text: Remote::new("x".repeat(text_len)),
            thread: None,
            reactions: Vec::new(),
            edited: false,
        })
        .collect()
}

fn put(store: &mut Store, t: &Target, n: u64, len: usize, seen: u64) {
    let p = Page { target: t.clone(), messages: messages(n, len), complete: false, users: Vec::new(), cut: false };
    store.put_page(&p, None, seen).unwrap();
}

fn kept(store: &Store, t: &Target) -> usize {
    store.newest_page(t, 10_000).unwrap().map_or(0, |p| p.messages.len())
}

#[test]
fn a_conversation_keeps_its_newest_messages_up_to_the_cap() {
    let mut store = writer("prune-cap");
    let (a, b) = (other("T1", "CA"), other("T1", "CB"));
    put(&mut store, &a, 250, 10, 100);
    put(&mut store, &b, 50, 10, 100);
    let caps = Caps { per_conversation: 200, ..Caps::default() };
    let out = store.prune(caps, &HashSet::new(), 100).unwrap();
    assert_eq!((kept(&store, &a), kept(&store, &b)), (200, 50));
    assert_eq!(out, Pruned { conversations: 0, messages: 50 });
    let newest = store.newest_page(&a, 1).unwrap().unwrap().messages[0].ts;
    assert_eq!(newest, Ts(250 * 1_000_000), "the oldest went, not the newest");
    assert_eq!(store.newest_page(&a, 10_000).unwrap().unwrap().messages[0].ts, Ts(51 * 1_000_000));
}

#[test]
fn conversations_not_opened_for_the_days_allowed_go() {
    let mut store = writer("prune-age");
    let now = 100 * DAY;
    let (old, recent) = (other("T1", "COLD"), other("T1", "CNEW"));
    put(&mut store, &old, 5, 10, now - 31 * DAY);
    put(&mut store, &recent, 5, 10, now - 29 * DAY);
    let out = store.prune(Caps::default(), &HashSet::new(), now).unwrap();
    assert_eq!((kept(&store, &old), kept(&store, &recent)), (0, 5));
    assert_eq!(out, Pruned { conversations: 1, messages: 5 });
    // Opened again (a page kept) counts as seen.
    put(&mut store, &recent, 5, 10, now + 20 * DAY);
    store.prune(Caps::default(), &HashSet::new(), now + 31 * DAY).unwrap();
    assert_eq!(kept(&store, &recent), 5);
}

#[test]
fn over_the_size_the_conversations_used_longest_ago_go_first_until_it_fits() {
    let mut store = writer("prune-size");
    let convs: Vec<Target> = (0..6).map(|i| other("T1", &format!("C{i}"))).collect();
    for (i, t) in convs.iter().enumerate() {
        put(&mut store, t, 100, 2_000, 1_000 + i as u64);
    }
    let before = store.used_bytes().unwrap();
    assert!(before > 1_000_000, "{before}");
    let cap = before / 2;
    let out = store.prune(Caps { max_bytes: cap, ..Caps::default() }, &HashSet::new(), 1_010).unwrap();
    assert!(store.used_bytes().unwrap() <= cap, "{} > {cap}", store.used_bytes().unwrap());
    let left: Vec<usize> = convs.iter().map(|t| kept(&store, t)).collect();
    // The oldest went, in order, and no more than needed.
    let gone = left.iter().take_while(|&&n| n == 0).count();
    assert!(gone >= 1 && left[gone..].iter().all(|&n| n == 100), "{left:?}");
    assert_eq!(out.conversations, gone);
    assert!(gone < 6, "the newest still fit: {left:?}");
}

#[test]
fn a_conversation_shown_is_never_pruned_by_any_cap() {
    let mut store = writer("prune-protect");
    let now = 100 * DAY;
    let shown = other("T1", "CSHOWN");
    let rest: Vec<Target> = (0..4).map(|i| other("T1", &format!("C{i}"))).collect();
    // The one shown is the oldest, the longest and the largest of all.
    put(&mut store, &shown, 300, 3_000, now - 90 * DAY);
    for t in &rest {
        put(&mut store, t, 50, 3_000, now - DAY);
    }
    let protect: HashSet<Target> = [shown.clone()].into();
    let caps = Caps { per_conversation: 200, max_bytes: 64 * 1024, max_days: 30 };
    store.prune(caps, &protect, now).unwrap();
    assert_eq!(kept(&store, &shown), 300, "untouched");
    assert!(rest.iter().all(|t| kept(&store, t) == 0), "the others made what room there was");
    // Not shown any more: the next prune applies to it as to any other.
    store.prune(caps, &HashSet::new(), now).unwrap();
    assert_eq!(kept(&store, &shown), 0);
}

#[test]
fn a_thread_in_protect_protects_nothing_and_a_read_only_cache_prunes_nothing() {
    let mut store = writer("prune-misc");
    let a = other("T1", "CA");
    put(&mut store, &a, 10, 10, 0);
    let thread =
        Target::Thread { workspace: a.workspace().clone(), conversation: a.conversation().clone(), thread: Ts(1) };
    store.prune(Caps::default(), &[thread].into(), 100 * DAY).unwrap();
    assert_eq!(kept(&store, &a), 0, "too old, and a thread target is not its conversation");
    let dir = store.dir().parent().unwrap().to_path_buf();
    put(&mut store, &a, 10, 10, 0);
    let mut reader = Store::open(&dir).unwrap().store;
    assert_eq!(reader.prune(Caps::default(), &HashSet::new(), 100 * DAY).unwrap(), Pruned::default());
    assert_eq!(kept(&store, &a), 10);
}

fn pragma(store: &Store, name: &str) -> i64 {
    store.conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get(0)).unwrap()
}

#[test]
fn a_prune_is_one_step_and_its_freed_pages_go_back_a_few_at_a_time() {
    let mut store = writer("prune-vacuum");
    let convs: Vec<Target> = (0..4).map(|i| other("T1", &format!("C{i}"))).collect();
    for (i, t) in convs.iter().enumerate() {
        put(&mut store, t, 100, 2_000, 1_000 + i as u64);
    }
    let file_pages = pragma(&store, "page_count");
    store.prune(Caps { max_bytes: 1, ..Caps::default() }, &HashSet::new(), 1_010).unwrap();
    assert!(convs.iter().all(|t| kept(&store, t) == 0));
    // The prune itself gives nothing back: the file keeps its size until the vacuum steps.
    assert_eq!(pragma(&store, "page_count"), file_pages);
    let free = pragma(&store, "freelist_count");
    assert!(free > 20, "{free}");
    let mut steps = 0;
    while store.vacuum(8).unwrap() {
        steps += 1;
        assert_eq!(pragma(&store, "freelist_count"), free - 8 * steps, "at most 8 pages a step");
    }
    assert_eq!(pragma(&store, "freelist_count"), 0, "every freed page went back in the end");
    assert!(pragma(&store, "page_count") < file_pages);
    assert!(!store.vacuum(8).unwrap(), "nothing left: no step more");
}

#[test]
fn a_thread_is_capped_aged_and_protected_as_a_conversation_is() {
    let mut store = writer("prune-thread");
    let conv = other("T1", "C1");
    let thread = |ts| Target::Thread {
        workspace: conv.workspace().clone(),
        conversation: conv.conversation().clone(),
        thread: Ts(ts),
    };
    let (long, old, shown) = (thread(1), thread(2), thread(3));
    put(&mut store, &long, 50, 10, 100 * DAY);
    put(&mut store, &old, 5, 10, DAY);
    put(&mut store, &shown, 5, 10, DAY);
    let caps = Caps { per_conversation: 20, max_bytes: u64::MAX, max_days: 30 };
    store.prune(caps, &HashSet::from([shown.clone()]), 100 * DAY).unwrap();
    assert_eq!(kept(&store, &long), 20, "the newest 20 replies");
    assert_eq!(kept(&store, &old), 0, "not opened for 99 days");
    assert_eq!(kept(&store, &shown), 5, "a thread shown is never pruned");
}

/// The authors of a bot's names that no kept message names any more go; those still named stay.
#[test]
fn a_bot_s_names_no_message_names_go() {
    let mut store = writer("prune-bots");
    let a = other("T1", "CA");
    let bot = |id: &str| slakio_core::model::User {
        id: UserId::new(id),
        workspace: slakio_core::model::WorkspaceId::new("T1"),
        name: Remote::new(id.to_string()),
        display_name: Remote::new(id.to_string()),
        org: slakio_core::model::Org::Own,
        bot: true,
        presence: slakio_core::model::Presence::Unknown,
    };
    let mut kept_msgs = messages(3, 5);
    kept_msgs[2].user = UserId::new("BHOOK~0000000000000002");
    let users = (0..50).map(|i| bot(&format!("BHOOK~{i:016x}"))).chain([bot("U1")]).collect();
    let p = Page { target: a.clone(), messages: kept_msgs, complete: false, users, cut: false };
    store.put_page(&p, None, 100).unwrap();
    store.prune(Caps::default(), &HashSet::new(), 100).unwrap();
    let page = store.newest_page(&a, 10).unwrap().unwrap();
    let mut ids: Vec<String> = page.users.iter().map(|u| u.id.to_string()).collect();
    ids.sort();
    assert_eq!(ids, ["BHOOK~0000000000000002", "U1"]);
    let rows: i64 = store.conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 2, "49 names no message names are gone");
}
