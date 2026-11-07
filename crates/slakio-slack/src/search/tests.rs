use super::*;
use proptest::prelude::*;

const AT: Ts = Ts(1_767_603_600_000_100);

#[test]
fn a_reply_is_told_by_its_link_and_a_message_is_not() {
    let reply = "https://acme.slack.com/archives/C1/p1767603600000100?thread_ts=1767600000.000200&cid=C1";
    assert_eq!(thread_of(reply, "C1", AT), Some(Ts(1_767_600_000_000_200)));
    let no_cid = "https://acme.slack.com/archives/C1/p1767603600000100?thread_ts=1767600000.000200";
    assert_eq!(thread_of(no_cid, "C1", AT), Some(Ts(1_767_600_000_000_200)));
    // The thread's own message, a link to another conversation, a broken time, no query.
    let own = "https://acme.slack.com/archives/C1/p1767603600000100?thread_ts=1767603600.000100&cid=C1";
    assert_eq!(thread_of(own, "C1", AT), None);
    let other = "https://acme.slack.com/archives/C1/p1767603600000100?thread_ts=1767600000.000200&cid=C9";
    assert_eq!(thread_of(other, "C1", AT), None);
    let broken = "https://acme.slack.com/archives/C1/p1767603600000100?thread_ts=17676x0000.000200";
    assert_eq!(thread_of(broken, "C1", AT), None);
    assert_eq!(thread_of("https://acme.slack.com/archives/C1/p1767603600000100", "C1", AT), None);
    assert_eq!(thread_of("", "C1", AT), None);
}

#[test]
fn a_match_without_a_conversation_or_a_time_is_left_out() {
    let m = |ts: &str, channel: Option<&str>| wire::SearchMatch {
        ts: ts.into(),
        channel: channel.map(|id| wire::SearchChannel { id: id.into(), name: Some("general".into()) }),
        user: Some("U1".into()),
        username: Some("alex".into()),
        text: Some("the \u{E000}deploy\u{E001} went out".into()),
        permalink: None,
    };
    assert!(hit(m("1767603600.000100", None)).is_none());
    assert!(hit(m("soon", Some("C01"))).is_none());
    let h = hit(m("1767603600.000100", Some("C01"))).expect("a hit");
    assert_eq!((h.conversation.as_str(), h.ts, h.thread), ("C01", AT, None));
    assert_eq!(h.text.unsanitized(), "the \u{E000}deploy\u{E001} went out");
}

#[test]
fn ids_not_of_slack_s_shape_are_not_taken_and_a_long_text_is_cut() {
    let m = |channel: &str, user: &str, text: String| wire::SearchMatch {
        ts: "1767603600.000100".into(),
        channel: Some(wire::SearchChannel { id: channel.into(), name: Some("general".into()) }),
        user: Some(user.into()),
        username: Some("alex".into()),
        text: Some(text),
        permalink: None,
    };
    for bad in ["../C1", "C", "c123", "X123", "C12\u{1b}3", "C1 2", "", &format!("C{}", "1".repeat(40))] {
        assert!(hit(m(bad, "U1AB", "x".into())).is_none(), "{bad:?}");
    }
    for good in ["C0123ABC", "G01", "D9Z"] {
        assert!(hit(m(good, "U1AB", "x".into())).is_some(), "{good:?}");
    }
    for (user, kept) in [("U0123", "U0123"), ("W9A", "W9A"), ("B01", "B01"), ("u1", ""), ("U1\n", ""), ("../U1", "")] {
        assert_eq!(hit(m("C1A", user, "x".into())).unwrap().user.as_str(), kept, "{user:?}");
    }
    let long = "w".repeat(BLOCK_MAX_CHARS * 3);
    let h = hit(m("C1A", "U1", long)).unwrap();
    assert_eq!(h.text.unsanitized().chars().count(), BLOCK_MAX_CHARS);
}

proptest! {
    /// Whatever a link says, a thread found is a well-formed time and never the message itself.
    #[test]
    fn any_link_names_no_thread_but_a_real_one(link in any::<String>(), tail in "[a-z_=&.0-9?#C]{0,40}") {
        for l in [link.clone(), format!("https://acme.slack.com/archives/C1/p1?{tail}{link}")] {
            if let Some(t) = thread_of(&l, "C1", AT) {
                prop_assert_ne!(t, AT);
                prop_assert!(l.contains("thread_ts="));
            }
        }
    }
}
