use super::*;
use slakio_core::model::UserId;
use slakio_core::sanitize::Remote;

fn n(key: &str, at: u64) -> Notice {
    Notice {
        key: key.into(),
        kind: NoticeKind::Mention,
        at: Ts(at),
        conversation: ConversationId::new("C1"),
        ts: Ts(at),
        thread: None,
        user: UserId::new("U1"),
        text: None,
        reaction: None,
        unread: false,
    }
}

fn keys(n: &Notices) -> Vec<String> {
    (0..n.len()).map(|i| n.get(i).unwrap().1.key.clone()).collect()
}

const NOW: Ts = Ts(1_000);

#[test]
fn pages_merge_newest_first_and_a_first_page_read_again_keeps_the_older_ones() {
    let (a, b) = (WorkspaceId::new("TA"), WorkspaceId::new("TB"));
    let mut all = Notices::default();
    all.put(&a, true, vec![n("a3", 30), n("a1", 10)], NOW);
    all.put(&b, true, vec![n("b2", 20)], NOW);
    assert_eq!(keys(&all), ["a3", "b2", "a1"]);
    all.put(&a, false, vec![n("a1", 10), n("a0", 5)], NOW);
    assert_eq!(keys(&all), ["a3", "b2", "a1", "a0"], "each key once");
    // From its oldest notice on, the first page says what Slack holds: `a3` is gone; `a1` and
    // `a0`, older, stay; `a1` read again is listed once, as the page has it.
    let mut a1 = n("a1", 10);
    a1.unread = true;
    all.put(&a, true, vec![n("a4", 40), a1], NOW);
    assert_eq!(keys(&all), ["a4", "b2", "a1", "a0"]);
    assert!(all.get(2).unwrap().1.unread, "the page's copy");
    all.forget_listed(&a);
    assert_eq!(keys(&all), ["b2"]);
    all.forget(&b);
    assert!(all.is_empty());
}

#[test]
fn a_page_counts_only_what_it_listed_anew() {
    let a = WorkspaceId::new("TA");
    let mut all = Notices::default();
    assert_eq!(all.put(&a, true, vec![n("a3", 30), n("a2", 20)], NOW), 2);
    assert_eq!(all.put(&a, false, vec![n("a3", 30), n("a2", 20)], NOW), 0, "all held already");
    assert_eq!(all.put(&a, false, vec![], NOW), 0);
    all.put(&a, false, (0..NOTICES_MAX as u64).map(|i| n(&format!("k{i}"), 100 + i)).collect(), NOW);
    assert_eq!(all.len(), NOTICES_MAX);
    assert_eq!(all.put(&a, false, vec![n("old", 1)], NOW), 0, "cut at once at the cap");
}

#[test]
fn a_live_mention_is_listed_once_until_the_next_first_page() {
    let a = WorkspaceId::new("TA");
    let mut all = Notices::default();
    all.put(&a, true, vec![n("a1", 10)], NOW);
    let m = Message {
        ts: Ts(50),
        user: UserId::new("U2"),
        text: Remote::new("hi"),
        thread: None,
        reactions: Vec::new(),
        edited: false,
    };
    let c = ConversationId::new("C1");
    assert!(all.live(&a, &c, None, &m));
    // A first page that does not hold it yet keeps it, for a while.
    all.put(&a, true, vec![n("a1", 10)], Ts(50));
    assert_eq!(all.len(), 2, "kept: Slack may list it late");
    assert!(!all.live(&a, &c, None, &m), "once");
    assert!(!all.live(&a, &c, None, &Message { ts: Ts(10), ..m.clone() }), "listed already");
    let top = &all.get(0).unwrap().1;
    assert!(top.unread && top.ts == Ts(50) && top.key.starts_with(LIVE));
    all.put(&a, true, vec![n("a5", 50)], Ts(50));
    assert_eq!(keys(&all), ["a5", "a1"], "the page holds its message: listed once");
    // Not held, but long ago: it goes.
    let late = Message { ts: Ts(60), ..m };
    assert!(all.live(&a, &c, None, &late));
    let later = Ts(60 + u64::try_from(LIVE_KEEP.as_micros()).unwrap());
    all.put(&a, true, vec![n("a5", 50)], later);
    assert_eq!(keys(&all), ["a5", "a1"]);
}

#[test]
fn only_so_many_are_kept() {
    let a = WorkspaceId::new("TA");
    let mut all = Notices::default();
    all.put(&a, true, (0..NOTICES_MAX as u64 + 10).map(|i| n(&format!("k{i}"), i)).collect(), NOW);
    assert_eq!(all.len(), NOTICES_MAX);
    assert_eq!(all.get(NOTICES_MAX - 1).unwrap().1.at, Ts(10), "the oldest went");
}
