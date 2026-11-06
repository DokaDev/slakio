use super::*;
use proptest::prelude::*;

fn host(h: &str) -> Place {
    Place::Host(h.into())
}

#[test]
fn a_message_link_names_its_workspace_conversation_and_time() {
    let l = parse("https://acme.slack.com/archives/C0123ABCD/p1700000000123456").unwrap();
    assert_eq!(
        l,
        SlackLink {
            place: host("acme.slack.com"),
            conversation: "C0123ABCD".into(),
            message: Some(Ts(1_700_000_000_123_456)),
            thread: None
        }
    );
    // The host is read in lower case, a trailing slash and a fragment are nothing.
    let same = parse("HTTPS://ACME.Slack.COM/archives/C0123ABCD/p1700000000123456/#x").unwrap();
    assert_eq!(same, l);
}

#[test]
fn a_reply_link_names_its_thread() {
    let url = "https://acme.slack.com/archives/C0123ABCD/p1700000100000200?thread_ts=1700000000.123456&cid=C0123ABCD";
    let l = parse(url).unwrap();
    assert_eq!(l.message, Some(Ts(1_700_000_100_000_200)));
    assert_eq!(l.thread, Some(Ts(1_700_000_000_123_456)));
    // `cid` may be missing or first; another conversation's, or the message itself, is no thread.
    let first = "https://acme.slack.com/archives/C0123ABCD/p1700000100000200?cid=C0123ABCD&thread_ts=1700000000.123456";
    assert_eq!(parse(first).unwrap().thread, Some(Ts(1_700_000_000_123_456)));
    let other = "https://acme.slack.com/archives/C0123ABCD/p1700000100000200?thread_ts=1700000000.123456&cid=C999";
    assert_eq!(parse(other).unwrap().thread, None);
    let itself = "https://acme.slack.com/archives/C0123ABCD/p1700000100000200?thread_ts=1700000100.000200";
    assert_eq!(parse(itself).unwrap().thread, None);
}

#[test]
fn conversation_enterprise_and_web_client_links() {
    let c = parse("https://acme.slack.com/archives/G0AB12CD").unwrap();
    assert_eq!((c.place, c.conversation.as_str(), c.message), (host("acme.slack.com"), "G0AB12CD", None));
    let e = parse("https://big-org.enterprise.slack.com/archives/C0AB12CD/p1700000000000001").unwrap();
    assert_eq!(e.place, Place::Enterprise("big-org.enterprise.slack.com".into()));
    let a = parse("https://app.slack.com/client/T0123/D0456/").unwrap();
    assert_eq!((a.place, a.conversation.as_str()), (Place::Team("T0123".into()), "D0456"));
}

#[test]
fn hostile_and_foreign_addresses_are_not_slack_links() {
    for url in [
        // Not Slack's host.
        "https://acme.slack.com.evil.test/archives/C0123ABCD/p1700000000123456",
        "https://evilslack.com/archives/C0123ABCD/p1700000000123456",
        "https://slack.com/archives/C0123ABCD/p1700000000123456",
        "https://acme.slack.co/archives/C0123ABCD/p1700000000123456",
        "https://evil.test/acme.slack.com/archives/C0123ABCD/p1700000000123456",
        "https://acme.slack.com@evil.test/archives/C0123ABCD/p1700000000123456",
        // A user before Slack's host (written in parts: the address alone reads as an e-mail).
        concat!("https://evil.test", "@", "acme.slack.com/archives/C0123ABCD/p1700000000123456"),
        "https://acme.slack.com:8443/archives/C0123ABCD/p1700000000123456",
        "https://a.b.slack.com/archives/C0123ABCD",
        "https://-acme.slack.com/archives/C0123ABCD",
        concat!("https://fi", "les.slack.com/archives/C0123ABCD"),
        "https://api.slack.com/archives/C0123ABCD",
        "https://app.slack.com/archives/C0123ABCD",
        "https://acme.slack.com%2eevil.test/archives/C0123ABCD",
        // Not https, or not a web address at all.
        "http://acme.slack.com/archives/C0123ABCD/p1700000000123456",
        "slack://channel?team=T1&id=C0123ABCD",
        "javascript:alert(1)//acme.slack.com/archives/C0123ABCD",
        "https://acme.slack.com/archives/C0123ABCD/p1700000000123456 x",
        // Not the shape of a Slack link.
        "https://acme.slack.com/",
        "https://acme.slack.com/archives",
        "https://acme.slack.com/archives/c0123abcd",
        "https://acme.slack.com/archives/X0123ABCD",
        "https://acme.slack.com/archives/C1",
        "https://acme.slack.com/archives/C0123ABCD/p170000000012345",
        "https://acme.slack.com/archives/C0123ABCD/p17000000001234567",
        "https://acme.slack.com/archives/C0123ABCD/1700000000123456",
        "https://acme.slack.com/archives/C0123ABCD/p1700000000.12345",
        "https://acme.slack.com/archives/C0123ABCD/p+700000000123456",
        "https://acme.slack.com/archives/C0123ABCD/p1700000000123456/more",
        "https://acme.slack.com/archives/C0123ABCD/../../evil",
        "https://acme.slack.com/messages/C0123ABCD",
        "https://acme.slack.com/archives/C0123%41BCD",
        "https://app.slack.com/client/C0123/C0456",
        "https://app.slack.com/client/T0123",
    ] {
        assert_eq!(parse(url), None, "{url}");
    }
}

#[test]
fn a_thread_time_must_read_as_one() {
    let at = |q: &str| parse(&format!("https://acme.slack.com/archives/C0123ABCD/p1700000100000200?thread_ts={q}"));
    for bad in ["1700000000", "1700000000.12345", "1700000000.1234567", "x.123456", ".123456", "17000000001.000000"] {
        assert_eq!(at(bad).unwrap().thread, None, "{bad}");
    }
    assert_eq!(at("1.000001").unwrap().thread, Some(Ts(1_000_001)));
    // A conversation link has no thread.
    assert_eq!(parse("https://acme.slack.com/archives/C0123ABCD?thread_ts=1.000001").unwrap().thread, None);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Whatever text: no panic, and a link read is always on one of Slack's hosts, with ids of
    /// Slack's shape.
    #[test]
    fn any_text_is_read_safely(s in ".{0,200}") {
        if let Some(l) = parse(&s) {
            let host_ok = match &l.place {
                Place::Host(h) | Place::Enterprise(h) => h.ends_with(".slack.com") && !h.contains(['@', ':']),
                Place::Team(t) => t.starts_with(['T', 'E']),
            };
            prop_assert!(host_ok, "{s:?} {l:?}");
            prop_assert!(l.conversation.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()));
        }
    }

    /// A message link made of any name, id and 16 digits reads back to them; any host that only
    /// ends in or starts with Slack's is never one.
    #[test]
    fn links_read_back_and_lookalike_hosts_never_do(
        name in "[a-z0-9][a-z0-9-]{0,20}[a-z0-9]",
        conv in "[CGD][A-Z0-9]{2,20}",
        ts in 0u64..10_000_000_000_000_000,
        evil in "[a-z]{1,10}",
    ) {
        prop_assume!(!NOT_WORKSPACES.contains(&name.as_str()));
        let url = format!("https://{name}.slack.com/archives/{conv}/p{ts:016}");
        let l = parse(&url).unwrap();
        prop_assert_eq!(l.place, Place::Host(format!("{name}.slack.com")));
        prop_assert_eq!(l.conversation, conv.clone());
        prop_assert_eq!(l.message, Some(Ts(ts)));
        for lookalike in [
            format!("https://{name}.slack.com.{evil}.test/archives/{conv}/p{ts:016}"),
            format!("https://{name}.slack.com{evil}/archives/{conv}/p{ts:016}"),
            format!("https://{evil}.test/{name}.slack.com/archives/{conv}/p{ts:016}"),
            format!("https://{name}.slack.com@{evil}.test/archives/{conv}/p{ts:016}"),
        ] {
            prop_assert_eq!(parse(&lookalike), None, "{}", lookalike);
        }
    }
}
