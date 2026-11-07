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
    assert!(hit(m("soon", Some("C1"))).is_none());
    let h = hit(m("1767603600.000100", Some("C1"))).expect("a hit");
    assert_eq!((h.conversation.as_str(), h.ts, h.thread), ("C1", AT, None));
    assert_eq!(h.text.unsanitized(), "the \u{E000}deploy\u{E001} went out");
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
