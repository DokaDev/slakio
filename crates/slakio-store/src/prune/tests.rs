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
    let p = Page { target: t.clone(), messages: messages(n, len), complete: false, users: Vec::new() };
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
    let dir = store.dir().to_path_buf();
    put(&mut store, &a, 10, 10, 0);
    let mut reader = Store::open(&dir).unwrap().store;
    assert_eq!(reader.prune(Caps::default(), &HashSet::new(), 100 * DAY).unwrap(), Pruned::default());
    assert_eq!(kept(&store, &a), 10);
}
