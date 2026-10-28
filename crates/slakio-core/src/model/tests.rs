use super::*;

#[test]
fn a_ts_is_written_the_way_slack_writes_it() {
    let ts = Ts(1_767_603_600_000_100);
    assert_eq!(ts.to_string(), "1767603600.000100");
    assert_eq!(ts.secs(), 1_767_603_600);
    assert!(Ts(1) < Ts(2));
}

#[test]
fn dms_and_group_dms_are_dms() {
    let c = |kind| Conversation {
        id: ConversationId::new("C1"),
        workspace: WorkspaceId::new("T1"),
        kind,
        name: "x".into(),
        section: SectionId::new("S1"),
        external: false,
        muted: false,
        unread: 0,
        mentions: 0,
    };
    assert!(!c(ConversationKind::Channel { private: false }).is_dm());
    assert!(c(ConversationKind::Dm { user: UserId::new("U1") }).is_dm());
    assert!(c(ConversationKind::GroupDm { users: vec![] }).is_dm());
}

#[test]
fn muted_channels_badge_mentions_only_and_muted_dms_nothing() {
    let c = |kind, muted| Conversation {
        id: ConversationId::new("C1"),
        workspace: WorkspaceId::new("T1"),
        kind,
        name: "x".into(),
        section: SectionId::new("S1"),
        external: false,
        muted,
        unread: 5,
        mentions: 2,
    };
    let channel = ConversationKind::Channel { private: false };
    assert_eq!(c(channel.clone(), false).badges(), (5, 2));
    assert_eq!(c(channel, true).badges(), (0, 2), "a mention of the user, never plain unread");
    assert_eq!(c(ConversationKind::Dm { user: UserId::new("U1") }, true).badges(), (0, 0));
    assert_eq!(c(ConversationKind::GroupDm { users: vec![] }, true).badges(), (0, 0));
    assert_eq!(c(ConversationKind::Dm { user: UserId::new("U1") }, false).badges(), (5, 2));
}
