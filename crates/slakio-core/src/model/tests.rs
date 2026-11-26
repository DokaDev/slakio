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
        closed: false,
        muted: false,
        unread: 0,
        mentions: 0,
        latest: None,
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
        closed: false,
        muted,
        unread: 5,
        mentions: 2,
        latest: None,
    };
    let channel = ConversationKind::Channel { private: false };
    assert_eq!(c(channel.clone(), false).badges(), (5, 2));
    assert_eq!(c(channel, true).badges(), (0, 2), "a mention of the user, never plain unread");
    assert_eq!(c(ConversationKind::Dm { user: UserId::new("U1") }, true).badges(), (0, 0));
    assert_eq!(c(ConversationKind::GroupDm { users: vec![] }, true).badges(), (0, 0));
    assert_eq!(c(ConversationKind::Dm { user: UserId::new("U1") }, false).badges(), (5, 2));
}

#[test]
fn a_notice_maps_only_the_texts_it_carries() {
    use super::System;
    let renamed: System = System::Renamed { old: Remote::new("a\u{1b}[2J"), new: Remote::new("b") };
    let shown = renamed.map(Remote::line);
    let old = Remote::new("a\u{1b}[2J").line().as_str().to_string();
    assert!(!old.contains('\u{1b}'), "sanitised: {old:?}");
    assert_eq!(shown.map(|s| s.as_str().to_string()), System::Renamed { old, new: "b".to_string() });
    assert_eq!(System::<Remote>::Joined.map(Remote::line), System::Joined);
}
