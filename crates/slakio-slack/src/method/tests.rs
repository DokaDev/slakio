use super::*;

#[test]
fn only_listed_names_parse_and_every_listed_one_does() {
    for m in Method::ALL {
        assert_eq!(Method::parse(m.name()), Ok(*m));
    }
    // Writes, enumerations and everything not listed are refused, whatever the case.
    for name in [
        "chat.postMessage",
        "chat.update",
        "chat.delete",
        "conversations.mark",
        "conversations.list",
        "conversations.members",
        "users.list",
        "users.setPresence",
        "users.profile.set",
        "reactions.add",
        "saved.add",
        "saved.list",
        "drafts.create",
        "stars.add",
        "Client.UserBoot",
        "",
    ] {
        assert!(matches!(Method::parse(name), Err(SlackError::NotAllowed { .. })), "{name}");
    }
}

#[test]
fn a_method_with_a_mode_has_a_reason() {
    for m in Method::ALL {
        assert!(!m.sends_mode() || m.reason().is_some(), "{m:?}");
    }
    assert_eq!(Method::ConversationsView.reason(), None);
    assert_eq!(Method::ClientUserBoot.reason(), Some("initial-data"));
    assert!(!Method::ClientUserBoot.sends_mode());
    assert!(Method::ClientCounts.sends_mode());
}
