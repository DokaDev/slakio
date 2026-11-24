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
    section_shape(&d, &page, "1");
    let t = text(&d, &path);
    for secret in ["L0SECRET", "Secret", "C0SECRET", "secret_emoji", "dGVhbTpDMDE"] {
        assert!(!t.contains(secret), "{secret} in {t}");
    }
    assert!(t.contains("sections parsed section page=1 n=0 type=standard name_len=16 is_custom=yes ids=2 ids_page=yes ids_cursor=yes next_id=yes"), "{t}");
    assert!(
        t.contains("n=1 type=direct_messages name_len=0 is_custom=no ids=0 ids_page=no ids_cursor=no next_id=no"),
        "{t}"
    );
    assert!(t.contains("sections field sort n=0 name=sort_order value=alphabetical"), "{t}");
    assert!(t.contains("sections field sort n=1 name=order_by value=text"), "{t}");
    assert!(t.contains("sections counted sections page=1 count=2"), "{t}");
    assert!(t.contains("sections field page_key name=response_metadata entries=1 page=1"), "{t}");
    assert!(t.contains("sections field section_key name=channel_ids_page entries=1 page=1"), "{t}");
    assert!(t.contains("sections field section_key name=next_channel_section_id entries=2 page=1"), "{t}");
    assert_eq!(next_cursor(&page), None);
    assert_eq!(next_cursor(&json!({"response_metadata": {"next_cursor": "abc"}})), Some("abc".to_string()));
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
}
