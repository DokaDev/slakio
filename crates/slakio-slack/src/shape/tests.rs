use super::*;
use serde_json::json;

fn log() -> (Diag, std::path::PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("slakio-shape-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("pictures.log");
    (Diag::to(path.clone()), path)
}

fn text(d: &Diag, path: &std::path::Path) -> String {
    d.flush();
    std::fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn a_section_says_its_shape_never_its_name_or_its_channels() {
    let (d, path) = log();
    let page = json!({
        "ok": true,
        "channel_sections": [
            {"channel_section_id": "L0SECRET1", "name": "Secret Team Room", "type": "standard",
             "emoji": "secret_emoji", "next_channel_section_id": "L0SECRET2", "sort_order": "alphabetical",
             "channel_ids_page": {"channel_ids": ["C0SECRET1", "C0SECRET2"], "count": 2, "cursor": "dGVhbTpDMDE="}},
            {"channel_section_id": "L0SECRET2", "name": "", "type": "direct_messages",
             "next_channel_section_id": null, "order_by": "Secret Free Text"}
        ],
        "response_metadata": {"next_cursor": ""}
    });
    section_shape(&d, &page, "1", None);
    let t = text(&d, &path);
    for secret in ["L0SECRET", "Secret", "C0SECRET", "secret_emoji", "dGVhbTpDMDE"] {
        assert!(!t.contains(secret), "{secret} in {t}");
    }
    assert!(t.contains("sections parsed section page=1 n=0 type=standard name_len=16 is_custom=yes ids=2 ids_page=yes ids_cursor=yes next_id=yes"), "{t}");
    assert!(
        t.contains("n=1 type=direct_messages name_len=0 is_custom=no ids=0 ids_page=no ids_cursor=no next_id=no"),
        "{t}"
    );
    assert!(t.contains("sections field sort page=1 n=0 name=sort_order value=alphabetical"), "{t}");
    assert!(t.contains("sections field sort page=1 n=1 name=order_by value=text"), "{t}");
    assert!(t.contains("sections counted sections page=1 count=2"), "{t}");
    assert!(t.contains("sections field page_key name=response_metadata entries=1 page=1"), "{t}");
    assert!(t.contains("sections field section_key name=channel_ids_page entries=1 page=1"), "{t}");
    assert!(t.contains("sections field section_key name=next_channel_section_id entries=2 page=1"), "{t}");
    assert!(t.contains("sections counted page page=1 count=- cursor=no next_cursor=no cursor_same=-"), "{t}");
    assert!(t.contains("sections field entities page=1 type=absent"), "{t}");
}

#[test]
fn the_start_bundle_says_which_fields_it_carries_and_never_their_values() {
    let (d, path) = log();
    let boot = json!({
        "ok": true,
        "self": {"id": "U0SECRET", "profile": {"image_72": "https://avatars.example/U0SECRET.png", "image_512": "x", "real_name": "Secret Person"}},
        "starred": ["C0SECRET"],
        "is_open": [],
        "ims": [{"id": "D0SECRET", "user": "U0SECRET", "is_open": true, "latest": "1767603600.000100"}, {"id": "D0SECRET2", "priority": 0.5}],
        "Weird Key": 1
    });
    boot_shape(&d, &boot);
    let t = text(&d, &path);
    for secret in ["SECRET", "avatars.example", "Secret Person", "1767603600"] {
        assert!(!t.contains(secret), "{secret} in {t}");
    }
    assert!(t.contains("boot-shape field present name=starred type=array count=1"), "{t}");
    assert!(t.contains("boot-shape field present name=is_open type=array count=0"), "{t}");
    assert!(t.contains("boot-shape field absent name=channel_sections"), "{t}");
    assert!(t.contains("boot-shape field ims_key name=latest entries=1"), "{t}");
    assert!(t.contains("boot-shape field ims_key name=priority entries=1"), "{t}");
    assert!(t.contains("self-photo parsed profile present=yes"), "{t}");
    assert!(t.contains("self-photo field image name=image_72"), "{t}");
    assert!(t.contains("self-photo field image name=image_512"), "{t}");
    boot_shape(&d, &json!([1]));
    assert!(text(&d, &path).contains("boot-shape failed parse"));
}

#[test]
fn names_and_values_are_told_only_when_plain() {
    assert_eq!(key("is_open"), "is_open");
    assert_eq!(key("Secret Name"), "other");
    assert_eq!(enum_like(&json!("standard")), "standard");
    assert_eq!(enum_like(&json!("Free text")), "text");
    assert_eq!(enum_like(&json!(3)), "number");
    let (long, longer) = ("a".repeat(31), "a".repeat(32));
    assert_eq!(key(&long), long);
    assert_eq!(key(&longer), "other", "the redaction would blank it");
    assert_eq!(enum_like(&json!(long)), long);
    assert_eq!(enum_like(&json!(longer)), "text");
}

#[test]
fn a_section_s_sort_fields_and_a_profile_s_pictures_are_told_at_most_forty_times() {
    let (d, path) = log();
    let mut section = serde_json::Map::new();
    let mut profile = serde_json::Map::new();
    for i in 0..500 {
        section.insert(format!("sort_{i}"), json!("alphabetical"));
        profile.insert(format!("image_{i}"), json!("x"));
    }
    section_shape(&d, &json!({"ok": true, "channel_sections": [section]}), "1", None);
    boot_shape(&d, &json!({"ok": true, "self": {"profile": profile}}));
    let text = text(&d, &path);
    assert_eq!(text.lines().filter(|l| l.contains("sections field sort")).count(), KEYS_MAX, "{text}");
    assert!(text.lines().filter(|l| l.contains("sections field sort")).all(|l| l.contains(" page=1 ")), "{text}");
    assert_eq!(text.lines().filter(|l| l.contains("self-photo field image")).count(), KEYS_MAX);
}

#[test]
fn the_start_bundle_s_own_profile_tells_its_field_names_and_photo_hosts_never_values() {
    let (d, path) = log();
    let original = "https://s3-us-west-2.amazonaws.com/slack-files2/avatars/2026-01-05/1234_SECRETFILE_original.jpg";
    let boot = json!({
        "ok": true,
        "self": {"id": "U0SECRET", "profile": {
            "image_original": original, "avatar_hash": "SECRETHASH", "is_custom_image": true,
            "real_name": "Secret Person", "status_text": "Secret Status"}},
    });
    boot_shape(&d, &boot);
    let t = text(&d, &path);
    for secret in ["SECRET", "Secret", "slack-files2", "2026-01-05", "1234_"] {
        assert!(!t.contains(secret), "{secret} in {t}");
    }
    for name in ["avatar_hash", "image_original", "is_custom_image", "real_name", "status_text"] {
        assert!(t.contains(&format!("self-photo field profile_key name={name} entries=1")), "{name}: {t}");
    }
    assert!(t.contains("self-photo counted image name=image_original entries=1"), "{t}");
    assert!(
        t.contains("self-photo counted image_host name=image_original host=s3-us-west-2.amazonaws.com urls=1"),
        "{t}"
    );
    assert!(t.contains("self-photo counted marks avatar_hash=1 is_custom_image=1 custom_true=1 of=1"), "{t}");
}

#[test]
fn a_profile_s_image_fields_are_counted_even_past_the_cap_of_names() {
    let (d, path) = log();
    let mut profile = serde_json::Map::new();
    // Sorted before `image_original`: the names told are cut before it.
    for i in 0..KEYS_MAX {
        profile.insert(format!("a_{i:02}"), json!(1));
    }
    profile.insert("image_original".into(), json!("https://s3-eu-west-1.amazonaws.com/x/y_original.png"));
    lookup_shape(&d, &json!({"ok": true, "results": [{"id": "U1", "profile": profile}]}));
    let t = text(&d, &path);
    assert!(!t.contains("profile_key name=image_original"), "the names are capped: {t}");
    assert_eq!(t.matches("avatar field profile_key").count(), KEYS_MAX, "{t}");
    assert!(t.contains("avatar counted image name=image_original entries=1"), "{t}");
    assert!(t.contains("avatar counted image_host name=image_original host=s3-eu-west-1.amazonaws.com urls=1"), "{t}");
    assert!(t.contains("avatar counted marks avatar_hash=0 is_custom_image=0 custom_true=0 of=1"), "{t}");
}

#[test]
fn entries_without_a_photo_are_tallied_once_not_skipped_silently() {
    let (d, path) = log();
    let persons: Vec<wire::Person> = serde_json::from_value(json!([
        {"id": "U1", "profile": {"real_name": "A", "avatar_hash": "h"}},
        {"id": "U2"},
        {"id": "U3", "profile": {"real_name": "C"}},
    ]))
    .unwrap();
    photos(&d, &persons, "lookup");
    photos(&d, &persons, "lookup");
    let t = text(&d, &path);
    assert_eq!(
        t.matches("avatar skipped no-photo from=lookup no_profile=1 no_photo=2 not_custom=0 of=3").count(),
        1,
        "{t}"
    );
    assert_eq!(t.matches("avatar counted decoded from=lookup results=3 pictured=0").count(), 2, "{t}");
}

#[test]
fn a_page_tells_its_count_its_cursor_and_its_entities_never_their_values() {
    let (d, path) = log();
    let page = json!({
        "ok": true, "count": 9, "cursor": "U0VDUkVUQ1VSU09S",
        "entities": {"C0SECRET1": {"id": "C0SECRET1", "name": "Secret Room"}, "apps": {"name": "x"}},
        "channel_sections": [{"channel_section_id": "L0SECRET1", "name": "", "type": "standard",
            "channel_ids_page": {"channel_ids": ["C0SECRET1"], "count": 7, "cursor": "U0VDUkVUSURT"}}],
    });
    section_shape(&d, &page, "2", Some("U0VDUkVUQ1VSU09S"));
    let t = text(&d, &path);
    for secret in ["SECRET", "Secret", "U0VDUkVU"] {
        assert!(!t.contains(secret), "{secret} in {t}");
    }
    assert!(t.contains("sections counted page page=2 count=9 cursor=yes next_cursor=no cursor_same=yes"), "{t}");
    assert!(t.contains("next_id=no ids_count=7"), "{t}");
    assert!(t.contains("sections field entities page=2 type=object count=2"), "{t}");
    assert!(t.contains("sections field entities_key name=apps entries=1 page=2"), "{t}");
    assert!(t.contains("sections field entities_key name=other entries=1 page=2"), "an id is never told: {t}");
    assert!(t.contains("sections field entity_key name=name entries=2 page=2"), "{t}");
}

#[test]
fn the_sidebar_preferences_and_the_counts_entries_tell_names_only() {
    let (d, path) = log();
    let boot = json!({"ok": true, "prefs": {
        "channel_sort": "alphabetical", "sidebar_behavior": "Secret Free Text", "dm_sort_order": true,
        "theme": "dark", "muted_channels": "C0SECRET"}});
    boot_shape(&d, &boot);
    let counts = json!({"ok": true, "channels": [{"id": "C0SECRET", "latest": "1767603600.000100", "has_unreads": true}],
        "ims": [{"id": "D0SECRET", "last_read": "1767603600.000100"}], "mpims": []});
    let raw = Raw { status: 200, retry_after: None, body: counts.to_string().into_bytes() };
    answered(&d, Method::ClientCounts, &raw);
    let t = text(&d, &path);
    for secret in ["SECRET", "Secret", "1767603600"] {
        assert!(!t.contains(secret), "{secret} in {t}");
    }
    assert!(t.contains("boot-shape field pref name=channel_sort value=alphabetical"), "{t}");
    assert!(t.contains("boot-shape field pref name=sidebar_behavior value=text"), "{t}");
    assert!(t.contains("boot-shape field pref name=dm_sort_order value=true"), "{t}");
    assert!(!t.contains("name=theme") && !t.contains("name=muted_channels"), "{t}");
    assert!(t.contains("counts-shape field channels_key name=latest entries=1"), "{t}");
    assert!(t.contains("counts-shape field ims_key name=last_read entries=1"), "{t}");
}

#[test]
fn a_photo_host_is_told_only_when_it_is_an_avatar_host_or_amazon_s_storage_of_a_region() {
    let at = |host: &str| format!("https://{host}/slack-files2/avatars/2026-01-05/a_original.png");
    for host in ["s3-us-west-2.amazonaws.com", "s3-eu-central-1.amazonaws.com", AVATAR_HOSTS_0] {
        assert_eq!(photo_host(&at(host)), host);
    }
    for host in [
        "workspace-secret.amazonaws.com",
        "s3-.amazonaws.com",
        "s3-us_west.amazonaws.com",
        "a.s3-us-west-2.amazonaws.com",
        "acme.example.com",
    ] {
        assert_eq!(photo_host(&at(host)), "other", "{host}");
    }
}

const AVATAR_HOSTS_0: &str = slakio_core::images::AVATAR_HOSTS[0];

#[test]
fn the_user_s_own_record_in_a_view_says_when_it_has_no_photo_of_their_own() {
    let (d, path) = log();
    let persons: Vec<wire::Person> = serde_json::from_value(json!([
        {"id": "U0", "profile": {"real_name": "Me", "image_72": format!("https://{AVATAR_HOSTS_0}/2026-01-05/a_72.png"),
            "is_custom_image": false}},
    ]))
    .unwrap();
    own_seen(&d, &persons, Some(&slakio_core::model::UserId::new("U0")));
    let t = text(&d, &path);
    assert!(t.contains("self-photo seen ok from=view photo=not_custom"), "{t}");
}

#[test]
fn a_start_bundle_without_the_user_s_profile_says_it_was_not_read() {
    for me in [json!({"id": "U0"}), json!({"id": "U0", "profile": "odd"})] {
        let (d, path) = log();
        own_profile(&d, &serde_json::from_value(me.clone()).unwrap());
        let t = text(&d, &path);
        assert!(t.contains("self-photo parsed bundle read=no"), "{me}: {t}");
        assert!(!t.contains("U0"), "{t}");
    }
}
