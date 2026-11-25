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

#[test]
fn a_person_shows_the_first_name_slack_gave() {
    let p: Person = serde_json::from_value(serde_json::json!({
        "id": "U1", "name": "minsu", "real_name": "Minsu Kim", "profile": {"display_name": "", "real_name": "Minsu K"}
    }))
    .unwrap();
    assert_eq!(p.shown_name(), Some("Minsu K"), "an empty display name is none");
    let bare: Person = serde_json::from_value(serde_json::json!({"id": "U2"})).unwrap();
    assert_eq!(bare.shown_name(), None);
    let view: View = serde_json::from_value(serde_json::json!({"users": [{"id": "U1"}, {"no_id": 1}]})).unwrap();
    assert_eq!(view.users.unwrap().len(), 1, "an unreadable person is left out");
}

fn profile(v: serde_json::Value) -> Profile {
    serde_json::from_value(v).unwrap()
}

#[test]
fn a_photo_is_picked_from_the_best_field_slack_gave() {
    let avatars = slakio_core::images::AVATAR_HOSTS[0];
    let at = |size: &str| format!("https://{avatars}/2026-01-05/1234_abc_{size}.png");
    let original = "https://s3-us-west-2.amazonaws.com/slack-files2/avatars/2026-01-05/1234_abc_original.png";
    let all = serde_json::json!({
        "image_24": at("24"), "image_32": at("32"), "image_48": at("48"), "image_72": at("72"),
        "image_192": at("192"), "image_512": at("512"), "image_1024": at("1024"), "image_original": original,
    });
    // Each field in turn, the better ones taken away: 72, 48, 192, 512, the original (as its
    // 72-pixel copy), 1024, 32, 24.
    let order =
        ["image_72", "image_48", "image_192", "image_512", "image_original", "image_1024", "image_32", "image_24"];
    let mut left = all.as_object().unwrap().clone();
    for field in order {
        let want = if field == "image_original" { at("72") } else { left[field].as_str().unwrap().to_string() };
        assert_eq!(profile(serde_json::Value::Object(left.clone())).photo(), Some((field, want)), "{field}");
        left.remove(field);
    }
    assert_eq!(profile(serde_json::Value::Object(left)).photo(), None);
}

#[test]
fn every_field_passes_the_host_check_and_an_original_is_taken_only_as_its_sized_copy() {
    let avatars = slakio_core::images::AVATAR_HOSTS[0];
    // A field on another host is passed over for the next.
    let p = profile(serde_json::json!({
        "image_72": "https://evil.example/2026-01-05/a_72.png",
        "image_48": format!("http://{avatars}/2026-01-05/a_48.png"),
        "image_original": "https://s3-us-west-2.amazonaws.com.evil.example/slack-files2/avatars/2026-01-05/a_original.png",
        "image_32": format!("https://{avatars}/2026-01-05/a_32.png"),
    }));
    assert_eq!(p.photo(), Some(("image_32", format!("https://{avatars}/2026-01-05/a_32.png"))));
    // An original that is not of the dated form, even on an avatar host: never taken as it is
    // (as uploaded it may be larger than slakio fetches); a sized copy is.
    let odd = format!("https://{avatars}/odd/a_original.png");
    let p = profile(serde_json::json!({"image_original": odd}));
    assert_eq!(p.photo(), None);
    let p = profile(serde_json::json!({"image_original": odd, "image_24": format!("https://{avatars}/a_24.png")}));
    assert_eq!(p.photo().map(|(f, _)| f), Some("image_24"));
    // Each field says how it is taken: the original only as its sized copy.
    let taken: Vec<(&str, Taken)> = p.candidates().into_iter().map(|(f, _, t)| (f, t)).collect();
    assert_eq!(taken.iter().filter(|(f, _)| *f == "image_original").count(), 1);
    assert!(taken.iter().all(|(f, t)| (*t == Taken::Sized) == (*f == "image_original")), "{taken:?}");
    // An original anywhere else is no photo.
    let p = profile(serde_json::json!({"image_original": "https://evil.example/2026-01-05/a_original.png"}));
    assert_eq!(p.photo(), None);
}

#[test]
fn a_person_without_a_photo_of_their_own_has_none_and_an_unknown_mark_changes_nothing() {
    let photo = format!("https://{}/2026-01-05/a_72.png", slakio_core::images::AVATAR_HOSTS[0]);
    let p = profile(serde_json::json!({"image_72": photo, "is_custom_image": false}));
    assert_eq!(p.avatar(), None, "Slack's default only: the initials are drawn");
    for mark in [serde_json::json!(true), serde_json::json!("false"), serde_json::json!(0), serde_json::json!(null)] {
        let p = profile(serde_json::json!({"image_72": photo, "is_custom_image": mark}));
        assert_eq!(p.avatar().as_deref(), Some(photo.as_str()), "{mark}");
    }
    assert_eq!(profile(serde_json::json!({"image_72": photo})).avatar().as_deref(), Some(photo.as_str()));
}

#[test]
fn a_wrongly_typed_field_of_a_profile_drops_neither_the_profile_nor_the_person() {
    let photo = format!("https://{}/2026-01-05/a_72.png", slakio_core::images::AVATAR_HOSTS[0]);
    let found: EdgeUsers = serde_json::from_value(serde_json::json!({"results": [
        {"id": "U1", "profile": {"display_name": 5, "real_name": "Minsu", "image_72": photo, "image_48": {"x": 1}}},
        {"id": "U2", "real_name": "Jiwoo", "profile": "odd"},
        {"id": "U3", "profile": {"real_name": ["x"], "image_72": 72, "image_original": false}},
    ]}))
    .unwrap();
    let people = found.results.unwrap();
    assert_eq!(people.len(), 3, "every person kept");
    assert_eq!(people[0].shown_name(), Some("Minsu"));
    assert_eq!(people[0].profile.as_ref().unwrap().avatar().as_deref(), Some(photo.as_str()));
    assert!(people[1].profile.is_none());
    assert_eq!(people[1].shown_name(), Some("Jiwoo"));
    assert_eq!(people[2].profile.as_ref().unwrap().avatar(), None);
}
