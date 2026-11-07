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

#[test]
fn pages_merge_newest_first_and_a_first_page_replaces() {
    let (a, b) = (WorkspaceId::new("TA"), WorkspaceId::new("TB"));
    let mut all = Notices::default();
    all.put(&a, true, vec![n("a3", 30), n("a1", 10)]);
    all.put(&b, true, vec![n("b2", 20)]);
    assert_eq!(keys(&all), ["a3", "b2", "a1"]);
    all.put(&a, false, vec![n("a1", 10), n("a0", 5)]);
    assert_eq!(keys(&all), ["a3", "b2", "a1", "a0"], "each key once");
    all.put(&a, true, vec![n("a4", 40)]);
    assert_eq!(keys(&all), ["a4", "b2"], "the first page says what Slack holds");
    all.forget(&b);
    assert_eq!(keys(&all), ["a4"]);
}

#[test]
fn a_live_mention_is_listed_once_until_the_next_first_page() {
    let a = WorkspaceId::new("TA");
    let mut all = Notices::default();
    all.put(&a, true, vec![n("a1", 10)]);
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
    assert!(!all.live(&a, &c, None, &m), "once");
    assert!(!all.live(&a, &c, None, &Message { ts: Ts(10), ..m.clone() }), "listed already");
    let top = &all.get(0).unwrap().1;
    assert!(top.unread && top.ts == Ts(50) && top.key.starts_with(LIVE));
    all.put(&a, true, vec![n("a5", 50)]);
    assert_eq!(keys(&all), ["a5"]);
}

#[test]
fn only_so_many_are_kept() {
    let a = WorkspaceId::new("TA");
    let mut all = Notices::default();
    all.put(&a, true, (0..NOTICES_MAX as u64 + 10).map(|i| n(&format!("k{i}"), i)).collect());
    assert_eq!(all.len(), NOTICES_MAX);
    assert_eq!(all.get(NOTICES_MAX - 1).unwrap().1.at, Ts(10), "the oldest went");
}
