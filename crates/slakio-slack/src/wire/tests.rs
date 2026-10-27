use super::*;

#[test]
fn timestamps_read_as_microseconds() {
    assert_eq!(ts("1767603600.000100"), Some(1_767_603_600_000_100));
    assert_eq!(ts("1767603600.5"), Some(1_767_603_600_500_000));
    assert_eq!(ts("1767603600"), Some(1_767_603_600_000_000));
    assert_eq!(ts("x.1"), None);
    assert_eq!(ts("1.1234567"), None);
}

#[test]
fn an_unreadable_entry_is_left_out_not_fatal() {
    let boot: UserBoot = serde_json::from_str(
        r#"{"self":{"id":"U1"},"team":{"id":"T1","extra":[1]},
            "channels":[{"id":"C1","name":"general"},{"name":"no id"},{"id":2},{"id":"C2","is_private":"yes"}],
            "ims":"not a list"}"#,
    )
    .unwrap();
    let ids: Vec<_> = boot.channels.unwrap().into_iter().map(|c| c.id).collect();
    assert_eq!(ids, ["C1"]);
    assert!(boot.ims.is_none(), "unknown, not empty");
    assert_eq!(boot.team.unwrap().name, None);
}
