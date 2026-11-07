use super::*;
use proptest::prelude::*;

fn item(v: serde_json::Value) -> Option<Notice> {
    notice(serde_json::from_value(v).ok()?)
}

#[test]
fn each_kind_shown_is_read_from_its_own_shape() {
    let m = |kind: &str| {
        serde_json::json!({"key": format!("k-{kind}"), "is_unread": true, "feed_ts": "1700000100.000200",
            "item": {"type": kind, "message": {"ts": "1700000000.000100", "channel": "C0AB", "author_user_id": "U0A", "thread_ts": "1699999999.000001"}}})
    };
    for (kind, want) in [
        ("at_user", NoticeKind::Mention),
        ("unjoined_channel_mention", NoticeKind::Mention),
        ("at_channel", NoticeKind::Everyone),
        ("at_everyone", NoticeKind::Everyone),
        ("at_user_group", NoticeKind::Group),
        ("keyword", NoticeKind::Keyword),
    ] {
        let n = item(m(kind)).unwrap();
        assert_eq!(n.kind, want);
        assert_eq!((n.conversation.as_str(), n.ts, n.user.as_str()), ("C0AB", Ts(1_700_000_000_000_100), "U0A"));
        assert_eq!(n.thread, Some(Ts(1_699_999_999_000_001)));
        assert_eq!(n.at, Ts(1_700_000_100_000_200));
        assert!(n.unread && n.text.is_none());
    }
    let thread = serde_json::json!({"key": "t", "item": {"type": "thread_v2", "bundle_info": {"unread_count": 2,
        "payload": {"thread_entry": {"channel_id": "C0AB", "thread_ts": "1.000001", "latest_ts": "2.000002"}}}}});
    let n = item(thread).unwrap();
    assert_eq!((n.kind, n.ts, n.thread, n.at), (NoticeKind::Thread, Ts(2_000_002), Some(Ts(1_000_001)), Ts(2_000_002)));
    assert!(!n.unread, "unknown is not unread");
    let reaction = serde_json::json!({"key": "r", "item": {"type": "message_reaction",
        "message": {"ts": "3.000003", "channel": "G0AB", "author_user_id": "U0ME"}, "reaction": {"user": "U0B", "name": "eyes"}}});
    let n = item(reaction).unwrap();
    assert_eq!((n.kind, n.user.as_str()), (NoticeKind::Reaction, "U0B"));
    assert_eq!(n.reaction.as_ref().map(|r| r.unsanitized().to_string()), Some("eyes".into()));
    let dm = serde_json::json!({"key": "d", "item": {"type": "dm", "bundle_info": {"payload": {"dm_entry":
        {"latest_message": {"ts": "4.000004", "channel": "D0AB", "author_user_id": "U0C"}}}}}});
    assert_eq!(item(dm).map(|n| (n.kind, n.conversation)), Some((NoticeKind::Dm, ConversationId::new("D0AB"))));
}

#[test]
fn kinds_not_shown_and_bad_shapes_are_left_out() {
    let base = |kind: &str, channel: &str, ts: &str, key: &str| serde_json::json!({"key": key, "item": {"type": kind, "message": {"ts": ts, "channel": channel}}});
    for (kind, channel, ts, key) in [
        ("saved_reminder", "C0AB", "1.000001", "k"),
        ("channel", "C0AB", "1.000001", "k"),
        ("list_user_mentioned", "C0AB", "1.000001", "k"),
        ("", "C0AB", "1.000001", "k"),
        ("at_user", "X0AB", "1.000001", "k"),
        ("at_user", "c0ab", "1.000001", "k"),
        ("at_user", "C0AB", "not a ts", "k"),
        ("at_user", "C0AB", "1.000001", ""),
    ] {
        assert_eq!(item(base(kind, channel, ts, key)), None, "{kind} {channel} {ts} {key:?}");
    }
    let long = "k".repeat(KEY_MAX + 1);
    assert_eq!(item(base("at_user", "C0AB", "1.000001", &long)), None);
    // A reaction without its reaction, a thread without its entry: nothing to show.
    let r = serde_json::json!({"key": "r", "item": {"type": "message_reaction", "message": {"ts": "1.000001", "channel": "C0AB"}}});
    assert_eq!(item(r), None);
    let t = serde_json::json!({"key": "t", "item": {"type": "thread_v2", "bundle_info": false}});
    assert_eq!(item(t), None);
    // An author not of Slack's shape is unknown; the message is no reply under itself.
    let odd = serde_json::json!({"key": "o", "item": {"type": "at_user",
        "message": {"ts": "1.000001", "channel": "C0AB", "author_user_id": "\u{1b}[31m", "thread_ts": "1.000001"}}});
    let n = item(odd).unwrap();
    assert!(n.user.as_str().is_empty() && n.thread.is_none());
}

#[test]
fn a_cursor_is_of_slacks_shape_or_none() {
    assert!(cursor_ok("dXNlcjpVMDYxTkZUVDI="));
    assert!(!cursor_ok(""));
    assert!(!cursor_ok("a b"));
    assert!(!cursor_ok("a\u{1b}b"));
    assert!(!cursor_ok(&"a".repeat(CURSOR_MAX + 1)));
}

proptest! {
    #[test]
    fn any_item_reads_without_a_panic(kind in ".{0,20}", channel in ".{0,40}", ts in ".{0,30}", key in ".{0,40}") {
        let v = serde_json::json!({"key": key, "item": {"type": kind, "message": {"ts": ts, "channel": channel},
            "bundle_info": {"payload": {"thread_entry": {"channel_id": channel, "thread_ts": ts, "latest_ts": ts}}}}});
        if let Some(n) = item(v) {
            prop_assert!(shaped(n.conversation.as_str(), b"CGD"));
            prop_assert!(!n.key.is_empty() && n.key.len() <= KEY_MAX);
        }
    }
}
