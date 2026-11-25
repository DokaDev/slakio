use super::*;
use crate::sidebar::Sidebar;
use serde_json::json;
use slakio_core::diag::Diag;
use slakio_core::model::SectionKind;

const NOW: u64 = 1_767_600_000;

/// The snapshot with no sections listed (the two default ones).
fn snapshot(b: wire::UserBoot, counts: Option<wire::Counts>, known: Vec<User>, now: u64) -> Snapshot {
    super::snapshot(b, counts, known, now, Sidebar::new("T1", None, &Diag::default()))
}

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
        avatar: None,
        no_photo: false,
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

fn me_with(profile: serde_json::Value) -> wire::UserBoot {
    let mut b = boot(json!({}), json!(null));
    b.me = serde_json::from_value(json!({"id": "U0", "name": "me", "profile": profile})).unwrap();
    b
}

fn person(id: &str, avatar: Option<&str>) -> User {
    User {
        id: UserId::new(id),
        workspace: WorkspaceId::new("T1"),
        name: Remote::new(id),
        display_name: Remote::new(id),
        org: Org::Own,
        bot: false,
        presence: Presence::Unknown,
        avatar: avatar.map(str::to_string),
        no_photo: false,
    }
}

#[test]
fn the_user_s_own_photo_comes_from_the_start_bundle_s_profile() {
    let host = slakio_core::images::AVATAR_HOSTS[0];
    let photo = format!("https://{host}/2020-01-01/0_72.png");
    let s = snapshot(me_with(json!({"image_72": photo})), None, Vec::new(), NOW);
    assert_eq!(s.users[0].id.as_str(), "U0");
    assert_eq!(s.users[0].avatar.as_deref(), Some(photo.as_str()));
    // A photo on any other host is none.
    let s = snapshot(me_with(json!({"image_72": "https://evil.example/0.png"})), None, Vec::new(), NOW);
    assert_eq!(s.users[0].avatar, None);
    // A profile that cannot be read is none, not a failed start.
    let s = snapshot(me_with(json!("odd")), None, Vec::new(), NOW);
    assert_eq!(s.users[0].avatar, None);
}

#[test]
fn the_user_s_own_photo_comes_from_the_start_bundle_s_original_as_its_72_pixel_copy() {
    let original = "https://s3-us-west-2.amazonaws.com/slack-files2/avatars/2026-01-05/1234_abc_original.jpg";
    let own = json!({"image_original": original, "avatar_hash": "0123abcd4567", "is_custom_image": true});
    let s = snapshot(me_with(own), None, Vec::new(), NOW);
    let want = format!("https://{}/2026-01-05/1234_abc_72.jpg", slakio_core::images::AVATAR_HOSTS[0]);
    assert_eq!(s.users[0].avatar.as_deref(), Some(want.as_str()));
    // No photo of their own: none.
    let s = snapshot(me_with(json!({"image_original": original, "is_custom_image": false})), None, Vec::new(), NOW);
    assert_eq!(s.users[0].avatar, None);
}

/// The user's own record says whether Slack said they have no photo of their own (so a photo
/// shown before goes): in the start bundle or as looked up; not said, it does not.
#[test]
fn the_user_s_own_record_says_when_slack_said_no_photo_of_their_own() {
    let not_custom = json!({"image_72": "https://evil.example/0.png", "is_custom_image": false});
    let s = snapshot(me_with(not_custom), None, Vec::new(), NOW);
    assert!(s.users[0].no_photo && s.users[0].avatar.is_none(), "the start bundle says so");
    let looked_up = User { no_photo: true, ..person("U0", None) };
    let s = snapshot(me_with(json!({})), None, vec![looked_up], NOW);
    assert!(s.users[0].no_photo, "the lookup says so");
    let s = snapshot(me_with(json!({})), None, vec![person("U0", None)], NOW);
    assert!(!s.users[0].no_photo, "not said");
}

#[test]
fn the_user_s_own_photo_looked_up_or_learned_is_kept_not_dropped() {
    let host = slakio_core::images::AVATAR_HOSTS[2];
    let learned = format!("https://{host}/img/avatars/ava_0010-72.png");
    let known = vec![person("U0", Some(&learned)), person("U1", None)];
    let s = snapshot(boot(json!({}), json!(null)), None, known.clone(), NOW);
    let me: Vec<&User> = s.users.iter().filter(|u| u.id.as_str() == "U0").collect();
    assert_eq!(me.len(), 1, "the user once");
    assert_eq!(me[0].avatar.as_deref(), Some(learned.as_str()));
    assert_eq!(me[0].org, Org::Own);
    // The start bundle's own photo comes first.
    let own = format!("https://{}/2020-01-01/0_72.png", slakio_core::images::AVATAR_HOSTS[0]);
    let s = snapshot(me_with(json!({"image_48": own})), None, known, NOW);
    assert_eq!(s.users[0].avatar.as_deref(), Some(own.as_str()));
}

#[test]
fn conversations_go_to_the_sections_slack_lists_a_section_of_the_user_s_own_first() {
    let listed: Vec<crate::sidebar::ChannelSection> = serde_json::from_value(json!([
        {"channel_section_id": "L2", "type": "channels", "name": "", "next_channel_section_id": "L3"},
        {"channel_section_id": "L1", "type": "standard", "name": "Mine", "next_channel_section_id": "L2",
         "channel_ids_page": {"channel_ids": ["C2", "D1"], "count": 2}},
        {"channel_section_id": "L3", "type": "direct_messages", "name": "DMs", "next_channel_section_id": null},
    ]))
    .unwrap();
    let sidebar = Sidebar::new("T1", Some(listed), &Diag::default());
    let s = super::snapshot(boot(json!({}), json!(null)), None, Vec::new(), NOW, sidebar);
    let names: Vec<&str> = s.sections.iter().map(|x| x.name.unsanitized()).collect();
    assert_eq!(names, ["Mine", "Channels", "DMs"], "in the chain's order, Slack's names else English ones");
    let section = |id| find(&s, id).section.as_str().to_string();
    assert_eq!(section("C2"), "T1:L1");
    assert!(s.sections.iter().all(|x| x.kind != SectionKind::Favorites), "no Starred listed");
    assert_eq!(section("D1"), "T1:L1", "a DM in a section of the user's own");
    assert_eq!(section("C1"), "T1:channels");
}

#[test]
fn the_conversations_slack_lists_as_starred_go_to_starred() {
    let mut b = boot(json!({}), json!(null));
    b.starred = Some(vec!["C1".to_string(), "D1".to_string(), "C9".to_string()]);
    let listed: Vec<crate::sidebar::ChannelSection> = serde_json::from_value(json!([
        {"channel_section_id": "LS", "type": "stars", "next_channel_section_id": "LH"},
        {"channel_section_id": "LH", "type": "channels", "next_channel_section_id": null},
    ]))
    .unwrap();
    let s = super::snapshot(b, None, Vec::new(), NOW, Sidebar::new("T1", Some(listed), &Diag::default()));
    assert_eq!(s.sections[0].name.unsanitized(), "Starred");
    assert_eq!(find(&s, "C1").section.as_str(), "T1:stars");
    assert_eq!(find(&s, "D1").section.as_str(), "T1:stars");
    assert_eq!(find(&s, "C2").section.as_str(), "T1:channels");
}

fn known_person(id: &str, shown: &str, bot: bool) -> User {
    User {
        id: UserId::new(id),
        workspace: WorkspaceId::new("T1"),
        name: Remote::new(shown.to_lowercase()),
        display_name: Remote::new(shown),
        org: Org::Own,
        bot,
        presence: Presence::Unknown,
        avatar: None,
        no_photo: false,
    }
}

fn placed() -> wire::UserBoot {
    serde_json::from_value(json!({
        "self": {"id": "U0", "name": "me"},
        "team": {"id": "T1", "name": "Team"},
        "starred": ["C1", "D2"],
        "is_open": ["G1", "D1"],
        "channels": [
            {"id": "C1", "name": "general"},
            {"id": "C2", "name": "stars-itself", "is_starred": true},
            {"id": "C3", "name": "partners", "is_ext_shared": true},
            {"id": "C4", "name": "mine", "is_starred": true},
            {"id": "C5", "name": "plain"},
            {"id": "G1", "name": "mpdm-me--ann--bob-1", "is_mpim": true, "members": ["U0", "U1", "U2"]},
            {"id": "G2", "name": "mpdm-me--cy-1", "is_mpim": true, "members": ["U0", "U3"]},
            {"id": "G3", "name": "mpdm-me--dee-1", "is_mpim": true, "is_archived": true, "members": ["U0", "U4"]},
        ],
        "ims": [
            {"id": "D1", "user": "U1", "is_open": true, "updated": 1_767_600_000_000_u64},
            {"id": "D2", "user": "U2", "is_starred": false},
            {"id": "D3", "user": "UB"},
            {"id": "D4", "user": "U4", "is_ext_shared": true},
            {"id": "D5", "user": "U5", "connected_team_ids": ["T9"]},
        ],
    }))
    .unwrap()
}

#[test]
fn starred_shared_app_and_open_group_conversations_go_to_their_sections() {
    let listed: Vec<crate::sidebar::ChannelSection> = serde_json::from_value(json!([
        {"channel_section_id": "LS", "type": "stars", "next_channel_section_id": "LC"},
        {"channel_section_id": "LC", "type": "standard", "name": "Mine", "next_channel_section_id": "LX",
         "channel_ids_page": {"channel_ids": ["C4"], "count": 1}},
        {"channel_section_id": "LX", "type": "slack_connect", "next_channel_section_id": "LH"},
        {"channel_section_id": "LH", "type": "channels", "next_channel_section_id": "LD"},
        {"channel_section_id": "LD", "type": "direct_messages", "next_channel_section_id": "LA"},
        {"channel_section_id": "LA", "type": "recent_apps", "next_channel_section_id": null},
    ]))
    .unwrap();
    let known = vec![known_person("U1", "Ann", false), known_person("UB", "Robot", true)];
    let sidebar = Sidebar::new("T1", Some(listed), &Diag::default());
    let s = super::snapshot(placed(), None, known.clone(), NOW, sidebar);
    let section = |id| find(&s, id).section.as_str().to_string();
    for (id, want) in [
        ("C1", "T1:stars"),
        ("C2", "T1:stars"),
        ("D2", "T1:stars"),
        ("C3", "T1:external"),
        ("D4", "T1:external"),
        ("D5", "T1:external"),
        ("C4", "T1:LC"),
        ("C5", "T1:channels"),
        ("D1", "T1:dms"),
        ("G1", "T1:dms"),
        ("D3", "T1:apps"),
    ] {
        assert_eq!(section(id), want, "{id}");
    }
    assert!(find(&s, "D4").external && find(&s, "D5").external, "shared DMs are Slack Connect");
    let group = find(&s, "G1");
    assert_eq!(group.kind, ConversationKind::GroupDm { users: vec![UserId::new("U1"), UserId::new("U2")] });
    assert_eq!(group.name.unsanitized(), "ann, bob", "Bob is not known: the handles, the user's left out");
    for gone in ["G2", "G3"] {
        assert!(s.conversations.iter().all(|c| c.id.as_str() != gone), "{gone}: closed or archived");
    }
    // Without Slack's sections, the group DM still shows, in Direct messages.
    let plain = snapshot(placed(), None, known, NOW);
    assert_eq!(find(&plain, "G1").section.as_str(), "T1:dms");
    assert_eq!(find(&plain, "C1").section.as_str(), "T1:channels", "no Starred listed: where it was");
}

#[test]
fn a_group_dm_is_named_by_its_people_when_all_are_known() {
    let users = [UserId::new("U1"), UserId::new("U2")];
    let known = [known_person("U1", "Ann", false), known_person("U2", "Bob", false)];
    assert_eq!(group_name(&users, &known, Some("mpdm-x--y-1"), None).as_deref(), Some("Ann, Bob"));
    assert_eq!(group_name(&users, &[], Some("mpdm-me--ann--bob-12"), Some("me")).as_deref(), Some("ann, bob"));
    assert_eq!(group_name(&users, &[], Some("odd"), None), None);
    assert_eq!(group_name(&[], &[], Some("mpdm-me-1"), Some("me")), None);
}
