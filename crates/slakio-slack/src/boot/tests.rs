use super::*;
use serde_json::json;

const NOW: u64 = 1_767_600_000;

fn boot(prefs: serde_json::Value, dnd: serde_json::Value) -> wire::UserBoot {
    serde_json::from_value(json!({
        "self": {"id": "U0", "name": "me"},
        "team": {"id": "T1", "name": "Team"},
        "channels": [
            {"id": "C1", "name": "general"},
            {"id": "C2", "name": "noise"},
            {"id": "C3", "name": "partners", "is_ext_shared": true},
            {"id": "C4", "name": "org-wide", "is_org_shared": true, "is_ext_shared": false},
        ],
        "ims": [{"id": "D1", "user": "U1"}],
        "prefs": prefs,
        "dnd": dnd,
    }))
    .unwrap()
}

fn counts() -> wire::Counts {
    serde_json::from_value(json!({
        "channels": [
            {"id": "C1", "has_unreads": true, "mention_count": 0},
            {"id": "C2", "has_unreads": true, "mention_count": 3},
            {"id": "C3", "has_unreads": false, "mention_count": 0},
        ],
        "ims": [{"id": "D1", "has_unreads": true, "mention_count": 2}],
    }))
    .unwrap()
}

fn find<'a>(s: &'a Snapshot, id: &str) -> &'a Conversation {
    s.conversations.iter().find(|c| c.id.as_str() == id).unwrap()
}

#[test]
fn counts_are_slack_s_and_unknown_counts_are_said() {
    let s = snapshot(boot(json!({}), json!(null)), Some(counts()), Vec::new(), NOW);
    assert!(!s.counts_failed);
    let got = |id| (find(&s, id).unread, find(&s, id).mentions);
    assert_eq!(got("C1"), (1, 0), "unread, no mention");
    assert_eq!(got("C2"), (3, 3));
    assert_eq!(got("C3"), (0, 0));
    assert_eq!(got("D1"), (2, 2), "a DM's messages are the user's");
    let failed = snapshot(boot(json!({}), json!(null)), None, Vec::new(), NOW);
    assert!(failed.counts_failed, "no counts: unknown, not zero");
}

#[test]
fn muted_conversations_come_from_either_spelling_of_the_prefs() {
    let all = json!({"channels": {"C2": {"muted": true}, "C1": {"muted": false}, "D1": {"muted": true}}}).to_string();
    let s = snapshot(
        boot(json!({"all_notifications_prefs": all, "muted_channels": "C1,C3"}), json!(null)),
        None,
        Vec::new(),
        NOW,
    );
    let muted: Vec<&str> = s.conversations.iter().filter(|c| c.muted).map(|c| c.id.as_str()).collect();
    assert_eq!(muted, ["C2", "C3", "D1"], "the newer spelling's false wins for C1");
    // Unreadable prefs mute nothing.
    let s = snapshot(boot(json!({"all_notifications_prefs": "{not json"}), json!(null)), None, Vec::new(), NOW);
    assert!(s.conversations.iter().all(|c| !c.muted));
}

#[test]
fn only_a_channel_shared_with_another_organization_is_slack_connect() {
    let s = snapshot(boot(json!({}), json!(null)), None, Vec::new(), NOW);
    let external: Vec<&str> = s.conversations.iter().filter(|c| c.external).map(|c| c.id.as_str()).collect();
    assert_eq!(external, ["C3"]);
    // A DM with a person of another team is too.
    let peer = User {
        id: UserId::new("U1"),
        workspace: WorkspaceId::new("T1"),
        name: Remote::new("p"),
        display_name: Remote::new("Partner"),
        org: Org::External(Remote::new("T9")),
        bot: false,
        presence: Presence::Unknown,
    };
    let s = snapshot(boot(json!({}), json!(null)), None, vec![peer], NOW);
    assert!(find(&s, "D1").external);
}

#[test]
fn the_user_s_do_not_disturb_is_read_from_the_start_bundle() {
    let me = |s: &Snapshot| s.users[0].presence;
    let on = json!({"dnd_enabled": true, "next_dnd_start_ts": NOW - 60, "next_dnd_end_ts": NOW + 60});
    assert_eq!(me(&snapshot(boot(json!({}), on), None, Vec::new(), NOW)), Presence::Dnd);
    let later = json!({"dnd_enabled": true, "next_dnd_start_ts": NOW + 60, "next_dnd_end_ts": NOW + 120});
    assert_eq!(me(&snapshot(boot(json!({}), later), None, Vec::new(), NOW)), Presence::Unknown);
    let snoozed = json!({"dnd_enabled": false, "snooze_enabled": true, "snooze_endtime": NOW + 5});
    assert_eq!(me(&snapshot(boot(json!({}), snoozed), None, Vec::new(), NOW)), Presence::Dnd);
    // Too little said: unknown, never "available".
    assert_eq!(wire::Dnd { dnd_enabled: Some(true), ..wire::Dnd::default() }.active(NOW), None);
    assert_eq!(me(&snapshot(boot(json!({}), json!("odd")), None, Vec::new(), NOW)), Presence::Unknown);
}
