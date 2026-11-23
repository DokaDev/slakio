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
