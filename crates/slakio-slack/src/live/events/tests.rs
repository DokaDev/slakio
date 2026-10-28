use super::*;
use serde_json::json;

fn me() -> UserId {
    UserId::new("U1")
}

fn of(v: &serde_json::Value) -> Frame {
    frame(&v.to_string(), Some(&me()))
}

#[test]
fn hello_goodbye_and_what_is_not_read() {
    assert_eq!(of(&json!({"type": "hello"})), Frame::Hello);
    assert_eq!(of(&json!({"type": "goodbye"})), Frame::Goodbye);
    for v in [
        json!({"type": "user_typing", "channel": "C1", "user": "U2"}),
        json!({"type": "presence_change", "user": "U2", "presence": "away"}),
        json!({"type": "reaction_added", "item": {"channel": "C1", "ts": "1.000001"}}),
        json!({"no": "type"}),
    ] {
        assert_eq!(of(&v), Frame::Other, "{v}");
    }
    assert_eq!(frame("{not json", None), Frame::Other);
}

#[test]
fn a_new_message_a_reply_and_a_broadcast() {
    let Frame::Message { conversation, thread, broadcast, changed, mention, message, .. } = of(&json!({
        "type": "message", "channel": "C1", "user": "U2", "text": "hi <@U1>", "ts": "1700000000.000100",
        "unknown_field": [1, 2]
    })) else {
        panic!("a message")
    };
    assert_eq!((conversation.as_str(), thread, broadcast, changed, mention), ("C1", None, false, false, true));
    assert_eq!((message.ts, message.user.as_str()), (Ts(1_700_000_000_000_100), "U2"));
    let Frame::Message { thread, broadcast, mention, .. } = of(&json!({
        "type": "message", "channel": "C1", "user": "U2", "text": "<@U12> not me", "ts": "1700000001.000000",
        "thread_ts": "1700000000.000100"
    })) else {
        panic!("a reply")
    };
    assert_eq!((thread, broadcast, mention), (Some(Ts(1_700_000_000_000_100)), false, false));
    let Frame::Message { thread, broadcast, .. } = of(&json!({
        "type": "message", "subtype": "thread_broadcast", "channel": "C1", "user": "U2", "text": "also",
        "ts": "1700000002.000000", "thread_ts": "1700000000.000100"
    })) else {
        panic!("a broadcast")
    };
    assert_eq!((thread, broadcast), (Some(Ts(1_700_000_000_000_100)), true));
    // A thread's own message names itself as its thread: not a reply.
    let Frame::Message { thread, .. } = of(&json!({
        "type": "message", "channel": "C1", "user": "U2", "text": "parent", "ts": "1700000000.000100",
        "thread_ts": "1700000000.000100"
    })) else {
        panic!("a parent")
    };
    assert_eq!(thread, None);
}

#[test]
fn everyone_mentions_count_and_a_longer_id_does_not() {
    for (text, mention) in [
        ("<@U1>", true),
        ("<@U1|minsu>", true),
        ("<@U12>", false),
        ("<!here> deploy", true),
        ("<!channel>", true),
        ("<!everyone>", true),
        ("U1 plain", false),
    ] {
        let v = json!({"type": "message", "channel": "C1", "user": "U2", "text": text, "ts": "1.000001"});
        let Frame::Message { mention: got, .. } = of(&v) else { panic!("{text}") };
        assert_eq!(got, mention, "{text}");
    }
}

#[test]
fn an_edit_and_a_reply_count_are_new_copies_and_a_deletion_names_its_thread() {
    let Frame::Message { changed, message, thread, .. } = of(&json!({
        "type": "message", "subtype": "message_changed", "channel": "C1", "hidden": true, "ts": "9.000000",
        "message": {"user": "U2", "text": "fixed", "ts": "1.000001", "edited": {"user": "U2", "ts": "9.000000"},
            "thread_ts": "0.500000"}
    })) else {
        panic!("an edit")
    };
    assert!(changed && message.edited);
    assert_eq!((message.ts, thread), (Ts(1_000_001), Some(Ts(500_000))));
    let Frame::Message { changed, message, .. } = of(&json!({
        "type": "message", "subtype": "message_replied", "channel": "C1", "hidden": true,
        "message": {"user": "U2", "text": "parent", "ts": "1.000001", "reply_count": 3, "latest_reply": "4.000000"}
    })) else {
        panic!("replies changed")
    };
    assert!(changed);
    assert_eq!(message.thread.map(|t| t.replies), Some(3));
    assert_eq!(
        of(&json!({"type": "message", "subtype": "message_deleted", "channel": "C1", "deleted_ts": "2.000000",
            "previous_message": {"ts": "2.000000", "thread_ts": "1.000001"}})),
        Frame::Deleted { conversation: "C1".into(), thread: Some(Ts(1_000_001)), ts: Ts(2_000_000) }
    );
    assert_eq!(
        of(&json!({"type": "message", "subtype": "message_deleted", "channel": "C1", "deleted_ts": "2.000000"})),
        Frame::Deleted { conversation: "C1".into(), thread: None, ts: Ts(2_000_000) }
    );
}

#[test]
fn what_cannot_be_placed_or_is_for_a_moment_is_left_out() {
    for v in [
        json!({"type": "message", "user": "U2", "text": "no channel", "ts": "1.000001"}),
        json!({"type": "message", "channel": "", "user": "U2", "text": "empty channel", "ts": "1.000001"}),
        json!({"type": "message", "channel": "C1", "user": "U2", "text": "no ts"}),
        json!({"type": "message", "channel": "C1", "user": "U2", "text": "e", "ts": "1.000001", "is_ephemeral": true}),
        json!({"type": "message", "subtype": "message_changed", "channel": "C1"}),
        json!({"type": "message", "subtype": "message_changed", "channel": "C1",
            "message": {"user": "U2", "text": "e", "ts": "1.000001", "is_ephemeral": true}}),
        json!({"type": "message", "subtype": "message_deleted", "channel": "C1"}),
    ] {
        assert_eq!(of(&v), Frame::Other, "{v}");
    }
}

#[test]
fn a_bot_is_named_as_it_posts() {
    let Frame::Message { message, bots, .. } = of(&json!({
        "type": "message", "subtype": "bot_message", "channel": "C1", "bot_id": "B1", "username": "deploy",
        "text": "done", "ts": "1.000001"
    })) else {
        panic!("a bot's message")
    };
    assert!(message.user.as_str().starts_with("B1~"));
    assert_eq!(bots.get(message.user.as_str()).map(|b| b.name.as_str()), Some("deploy"));
}

#[test]
fn read_elsewhere_carries_both_counts_or_is_left_out() {
    for kind in ["channel_marked", "im_marked", "group_marked", "mpim_marked"] {
        let v = json!({"type": kind, "channel": "D1", "ts": "1.000001", "unread_count_display": 2, "mention_count": 1});
        assert_eq!(of(&v), Frame::Marked { conversation: "D1".into(), unread: 2, mentions: 1 }, "{kind}");
    }
    // Unknown is not zero: without the counts it says nothing.
    assert_eq!(of(&json!({"type": "channel_marked", "channel": "C1", "ts": "1.000001"})), Frame::Other);
    let more = json!({"type": "channel_marked", "channel": "C1", "unread_count_display": 0, "mention_count": 3});
    assert_eq!(of(&more), Frame::Marked { conversation: "C1".into(), unread: 3, mentions: 3 });
}
