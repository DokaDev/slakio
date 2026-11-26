use super::*;

#[test]
fn at_most_the_names_cap_is_kept_and_long_or_odd_values_are_read_past() {
    let mut body = String::from("{\"ok\":true,\"emoji\":{");
    body.push_str(&format!("\"long\":\"{}\",", "a".repeat(2 * 1024 + 1)));
    body.push_str("\"number\":1,\"list\":[1,[2]],\"map\":{\"x\":{}},\"null\":null,\"bad name!\":\"alias:x\",");
    let many: Vec<String> = (0..100_000).map(|k| format!("\"e{k}\":\"alias:e0\"")).collect();
    body.push_str(&many.join(","));
    body.push_str("}}");
    let listed: Listed = serde_json::from_str(&body).expect("read");
    let names = listed.emoji.0;
    assert_eq!(names.len(), NAMES_MAX, "100,000 names: the cap kept");
    assert_eq!(names[0], ("e0".to_string(), "alias:e0".to_string()), "only the shortcodes with text");
    assert!(names.iter().all(|(_, v)| v.len() <= ADDRESS_MAX));
    let empty: Listed = serde_json::from_str("{\"ok\":true}").expect("read");
    assert!(empty.emoji.0.is_empty());
}

#[test]
fn korean_names_are_kept_and_names_hiding_characters_read_past() {
    let body = concat!(
        "{\"ok\":true,\"emoji\":{",
        "\"\u{C9DD}\u{C9DD}-\u{BCD1}\u{C544}\u{B9AC}\":\"alias:party\",",
        "\"\u{C9F1}\u{AD6C}_wow-2\":\"alias:\u{C9DD}\u{C9DD}-\u{BCD1}\u{C544}\u{B9AC}\",",
        "\"a\u{202E}b\":\"alias:party\",\"a\u{200D}b\":\"alias:party\",\"\u{E000}\":\"alias:party\"",
        "}}"
    );
    let listed: Listed = serde_json::from_str(body).expect("read");
    let names: Vec<&str> = listed.emoji.0.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["\u{C9DD}\u{C9DD}-\u{BCD1}\u{C544}\u{B9AC}", "\u{C9F1}\u{AD6C}_wow-2"]);
}

#[test]
fn only_an_answer_that_is_not_slack_s_json_is_told_as_not_slack_json() {
    let not = SlackError::Network { detail: crate::response::NOT_JSON.to_string() };
    assert_eq!(not_json(not), SlackError::Api { code: "not_slack_json".to_string() });
    // Another failure whose words name JSON stays what it is.
    let other = SlackError::Network { detail: "proxy said: JSON gateway timeout".to_string() };
    assert_eq!(not_json(other.clone()), other);
}

#[test]
fn a_list_past_the_names_cap_is_told_cut_and_both_forms_of_a_name_count_once() {
    let body = |extra: usize| {
        let (nfc, nfd) = ("\u{AC10}\u{C0AC}", "\u{1100}\u{1161}\u{11B7}\u{1109}\u{1161}");
        let mut names = vec![format!("\"{nfd}\":\"alias:tada\""), format!("\"{nfc}\":\"alias:tada\"")];
        names.extend((1..NAMES_MAX + extra).map(|k| format!("\"e{k}\":\"alias:tada\"")));
        format!("{{\"ok\":true,\"emoji\":{{{}}}}}", names.join(","))
    };
    let whole: Listed = serde_json::from_str(&body(0)).expect("read");
    assert_eq!(whole.emoji.0.len(), NAMES_MAX + 1, "both forms kept: one place");
    assert!(!whole.emoji.2, "every name kept");
    let cut: Listed = serde_json::from_str(&body(1)).expect("read");
    assert_eq!(cut.emoji.0.len(), NAMES_MAX + 1);
    assert!(cut.emoji.2, "one name past the cap: cut");
}
