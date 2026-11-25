use super::*;
use serde_json::json;

fn listed(v: serde_json::Value) -> Vec<ChannelSection> {
    serde_json::from_value(v).unwrap()
}

fn ids(list: &[ChannelSection]) -> Vec<&str> {
    list.iter().map(|s| s.channel_section_id.as_str()).collect()
}

fn kinds(s: &Sidebar) -> Vec<(SectionKind, &str, &str)> {
    s.sections.iter().map(|x| (x.kind, x.id.as_str(), x.name.unsanitized())).collect()
}

#[test]
fn the_chain_orders_the_sections_not_the_list() {
    let list = listed(json!([
        {"channel_section_id": "L3", "next_channel_section_id": null},
        {"channel_section_id": "L1", "next_channel_section_id": "L2"},
        {"channel_section_id": "L2", "next_channel_section_id": "L3"},
    ]));
    assert_eq!(ids(&order(list)), ["L1", "L2", "L3"]);
}

#[test]
fn a_cycle_ends_the_chain_and_the_sections_never_reached_follow_once_each() {
    let list = listed(json!([
        {"channel_section_id": "L9", "next_channel_section_id": "L8"},
        {"channel_section_id": "L1", "next_channel_section_id": "L2"},
        {"channel_section_id": "L2", "next_channel_section_id": "L1"},
        {"channel_section_id": "L8", "next_channel_section_id": "L9"},
        {"channel_section_id": "L1", "next_channel_section_id": null},
        {"channel_section_id": "L5", "next_channel_section_id": "L5"},
    ]));
    let ordered = order(list);
    assert_eq!(
        ids(&ordered),
        ["L5", "L9", "L1", "L2", "L8"],
        "L5 names only itself: the head; the rest as listed, L1 once"
    );
    assert_eq!(ordered.iter().filter(|s| s.channel_section_id == "L1").count(), 1);
    let all_named = listed(json!([
        {"channel_section_id": "L1", "next_channel_section_id": "L2"},
        {"channel_section_id": "L2", "next_channel_section_id": "L1"},
    ]));
    assert_eq!(ids(&order(all_named)), ["L1", "L2"], "every one named: the first listed leads");
    assert!(order(Vec::new()).is_empty());
}

#[test]
fn slack_s_types_map_to_kinds_and_the_others_are_not_shown() {
    let list = listed(json!([
        {"channel_section_id": "LT", "type": "temporary_channels", "next_channel_section_id": "LS"},
        {"channel_section_id": "LS", "type": "stars", "name": "", "next_channel_section_id": "LC"},
        {"channel_section_id": "LC", "type": "standard", "name": " Team ", "next_channel_section_id": "LX"},
        {"channel_section_id": "LX", "type": "slack_connect", "name": "Slack Connect", "next_channel_section_id": "LH"},
        {"channel_section_id": "LH", "type": "channels", "name": "Rooms", "next_channel_section_id": "LD"},
        {"channel_section_id": "LD", "type": "direct_messages", "next_channel_section_id": "LA"},
        {"channel_section_id": "LA", "type": "recent_apps", "name": "", "next_channel_section_id": "LF"},
        {"channel_section_id": "LF", "type": "salesforce_records", "next_channel_section_id": "LG"},
        {"channel_section_id": "LG", "type": "agents", "next_channel_section_id": "LU"},
        {"channel_section_id": "LU", "type": "something_new", "next_channel_section_id": "LH2"},
        {"channel_section_id": "LH2", "type": "channels", "name": "Again", "next_channel_section_id": null},
    ]));
    let s = Sidebar::new("T1", Some(list), &Diag::default());
    assert_eq!(
        kinds(&s),
        [
            (SectionKind::Favorites, "T1:stars", ""),
            (SectionKind::Custom, "T1:custom:LC", "Team"),
            (SectionKind::External, "T1:external", ""),
            (SectionKind::Channels, "T1:channels", ""),
            (SectionKind::DirectMessages, "T1:dms", ""),
            (SectionKind::Apps, "T1:apps", ""),
        ]
    );
}

#[test]
fn no_list_or_one_without_channels_keeps_the_two_default_sections() {
    let fallback = Sidebar::new("T1", None, &Diag::default());
    assert_eq!(
        kinds(&fallback),
        [(SectionKind::Channels, "T1:channels", ""), (SectionKind::DirectMessages, "T1:dms", "")]
    );
    assert_eq!(fallback.place("C1", Facts::default()).as_str(), "T1:channels");
    assert_eq!(fallback.place("D1", Facts { dm: true, ..Facts::default() }).as_str(), "T1:dms");
    let only_stars = listed(json!([{"channel_section_id": "LS", "type": "stars"}]));
    let s = Sidebar::new("T1", Some(only_stars), &Diag::default());
    assert_eq!(
        s.sections.iter().map(|x| x.kind).collect::<Vec<_>>(),
        [SectionKind::Favorites, SectionKind::Channels, SectionKind::DirectMessages]
    );
}

#[test]
fn a_section_of_the_user_s_own_lists_its_first_page_and_the_gap_is_told() {
    let dir = std::env::temp_dir().join(format!("slakio-sidebar-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("pictures.log");
    let diag = Diag::to(path.clone());
    let list = listed(json!([
        {"channel_section_id": "L1", "type": "standard", "name": "A", "next_channel_section_id": "L2",
         "channel_ids_page": {"channel_ids": ["C1", "C2"], "count": 5, "cursor": "Q1VSU09S"}},
        {"channel_section_id": "L2", "type": "standard", "name": "B", "next_channel_section_id": null,
         "channel_ids_page": {"channel_ids": ["C2", "C3"], "count": 2}},
    ]));
    let s = Sidebar::new("T1", Some(list), &diag);
    assert_eq!(s.place("C1", Facts::default()).as_str(), "T1:custom:L1");
    assert_eq!(s.place("C2", Facts::default()).as_str(), "T1:custom:L1", "the first section in order");
    assert_eq!(s.place("C3", Facts::default()).as_str(), "T1:custom:L2");
    assert_eq!(s.place("C9", Facts::default()).as_str(), "T1:channels", "beyond the first page: its default section");
    diag.flush();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("sections skipped more-ids n=0 ids=2 count=5"), "{text}");
    assert_eq!(text.matches("more-ids").count(), 1, "{text}");
    assert!(!text.contains("Q1VSU09S") && !text.contains("C1"), "{text}");
}

#[test]
fn a_starred_conversation_goes_to_starred_unless_in_a_section_of_the_user_s_own() {
    let list = listed(json!([
        {"channel_section_id": "LS", "type": "stars", "next_channel_section_id": "LC"},
        {"channel_section_id": "LC", "type": "standard", "name": "Mine", "next_channel_section_id": null,
         "channel_ids_page": {"channel_ids": ["C1"], "count": 1}},
    ]));
    let s = Sidebar::new("T1", Some(list), &Diag::default());
    let starred = Facts { starred: true, ..Facts::default() };
    assert_eq!(s.place("C1", starred).as_str(), "T1:custom:LC", "a section of the user's own first");
    assert_eq!(s.place("C2", starred).as_str(), "T1:stars");
    assert_eq!(s.place("D2", Facts { dm: true, starred: true, ..Facts::default() }).as_str(), "T1:stars");
    assert_eq!(s.place("D3", Facts { dm: true, ..Facts::default() }).as_str(), "T1:dms");
    // Without Slack listing Starred, a starred conversation stays where it was.
    let plain = Sidebar::new("T1", None, &Diag::default());
    assert_eq!(plain.place("C2", starred).as_str(), "T1:channels");
}

#[test]
fn a_conversation_goes_to_the_first_section_that_takes_it() {
    let list = listed(json!([
        {"channel_section_id": "LS", "type": "stars", "next_channel_section_id": "LC"},
        {"channel_section_id": "LC", "type": "standard", "name": "Mine", "next_channel_section_id": "LX",
         "channel_ids_page": {"channel_ids": ["C1"], "count": 1}},
        {"channel_section_id": "LX", "type": "slack_connect", "next_channel_section_id": "LA"},
        {"channel_section_id": "LA", "type": "recent_apps", "next_channel_section_id": null},
    ]));
    let s = Sidebar::new("T1", Some(list), &Diag::default());
    let all = Facts { dm: true, starred: true, external: true, app: true };
    assert_eq!(s.place("C1", all).as_str(), "T1:custom:LC", "a section of the user's own first");
    assert_eq!(s.place("D1", all).as_str(), "T1:stars");
    assert_eq!(s.place("D1", Facts { starred: false, ..all }).as_str(), "T1:external");
    assert_eq!(s.place("D1", Facts { dm: true, app: true, ..Facts::default() }).as_str(), "T1:apps");
    let channel = Facts { app: true, ..Facts::default() };
    assert_eq!(s.place("C2", channel).as_str(), "T1:channels", "only a DM is an app's");
    assert_eq!(s.place("D2", Facts { dm: true, ..Facts::default() }).as_str(), "T1:dms");
    // Without Slack listing them, shared and app conversations stay where they were.
    let plain = Sidebar::new("T1", None, &Diag::default());
    assert_eq!(plain.place("D1", all).as_str(), "T1:dms");
    assert_eq!(plain.place("C1", Facts { dm: false, ..all }).as_str(), "T1:channels");
}

#[test]
fn a_section_id_of_slack_s_never_takes_a_built_in_one_and_an_odd_one_is_not_shown() {
    let list = listed(json!([
        {"channel_section_id": "channels", "type": "standard", "name": "Mine", "next_channel_section_id": "dms",
         "channel_ids_page": {"channel_ids": ["C1"], "count": 1}},
        {"channel_section_id": "dms", "type": "standard", "name": "Also", "next_channel_section_id": "L:x",
         "channel_ids_page": {"channel_ids": ["D1"], "count": 1}},
        {"channel_section_id": "L:x", "type": "standard", "name": "Odd", "next_channel_section_id": "LONG",
         "channel_ids_page": {"channel_ids": ["C2"], "count": 1}},
        {"channel_section_id": "L0123456789012345678901234567890123", "type": "standard", "name": "Long",
         "channel_ids_page": {"channel_ids": ["C3"], "count": 1}},
        {"channel_section_id": "LH", "type": "channels", "next_channel_section_id": "LD"},
        {"channel_section_id": "LD", "type": "direct_messages"},
    ]));
    let s = Sidebar::new("T1", Some(list), &Diag::default());
    let ids: Vec<&str> = s.sections.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(ids, ["T1:custom:channels", "T1:custom:dms", "T1:channels", "T1:dms"]);
    let unique: HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len(), "no two sections share an id");
    assert_eq!(s.place("C1", Facts::default()).as_str(), "T1:custom:channels");
    assert_eq!(s.place("D1", Facts { dm: true, ..Facts::default() }).as_str(), "T1:custom:dms");
    assert_eq!(s.place("C2", Facts::default()).as_str(), "T1:channels", "an odd id: its default section");
    assert_eq!(s.place("C3", Facts::default()).as_str(), "T1:channels", "a long id: its default section");
    assert_eq!(s.place("C4", Facts::default()).as_str(), "T1:channels");
}
