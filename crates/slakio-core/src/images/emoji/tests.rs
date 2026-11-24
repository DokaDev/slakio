use super::*;
use crate::images::{EMOJI_HOSTS, Source};

fn pic(name: &str) -> String {
    format!("https://{}/T1/{name}/abc123.png", EMOJI_HOSTS[0])
}

fn list(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

fn get(resolved: &[(String, Custom)], name: &str) -> Option<Custom> {
    resolved.iter().find(|(n, _)| n == name).map(|(_, c)| c.clone())
}

#[test]
fn a_picture_on_the_emoji_host_is_kept_and_an_alias_chain_leads_to_it() {
    let blob = pic("party-blob");
    let r = resolve(&list(&[
        ("party-blob", &blob),
        ("shipit", "alias:party-blob"),
        ("ship2", "alias:shipit"),
        ("ship3", "alias:ship2"),
        ("ship4", "alias:ship3"),
    ]));
    for name in ["party-blob", "shipit", "ship2", "ship3", "ship4"] {
        assert_eq!(get(&r, name), Some(Custom::Picture(blob.clone())), "{name}");
    }
}

#[test]
fn an_alias_deeper_than_the_cap_draws_nothing_of_its_own() {
    let blob = pic("deep");
    let mut pairs = vec![("deep-0".to_string(), blob.clone())];
    for i in 1..=ALIAS_DEPTH + 1 {
        pairs.push((format!("deep-{i}"), format!("alias:deep-{}", i - 1)));
    }
    let r = resolve(&pairs);
    assert_eq!(get(&r, &format!("deep-{ALIAS_DEPTH}")), Some(Custom::Picture(blob)), "at the cap");
    assert_eq!(get(&r, &format!("deep-{}", ALIAS_DEPTH + 1)), None, "past it");
}

#[test]
fn a_cycle_draws_nothing() {
    let r = resolve(&list(&[("loop-a", "alias:loop-b"), ("loop-b", "alias:loop-a"), ("self", "alias:self")]));
    assert_eq!(r, Vec::new());
}

#[test]
fn an_alias_of_a_standard_emoji_draws_it() {
    let r = resolve(&list(&[("yes", "alias:white_check_mark"), ("yes2", "alias:yes"), ("bad", "alias:a:b")]));
    assert_eq!(get(&r, "yes"), Some(Custom::Standard("white_check_mark".into())));
    assert_eq!(get(&r, "yes2"), Some(Custom::Standard("white_check_mark".into())));
    assert_eq!(get(&r, "bad"), None, "not a shortcode");
}

#[test]
fn a_picture_anywhere_else_draws_nothing() {
    let host = EMOJI_HOSTS[0];
    let bad = [
        "https://evil.example/e.png".to_string(),
        format!("http://{host}/T1/e/a.png"),
        format!("https://{host}.evil.example/e.png"),
        format!("https://{host}@evil.example/e.png"),
        format!("https://evil@{host}/e.png"),
        format!("https://{host}:8443/e.png"),
        format!("https://{host}/e.png#x"),
        format!("https://{}/e.png", crate::images::AVATAR_HOSTS[0]),
        format!("https://{}/e.png", crate::model::FILE_HOST),
        "javascript:alert(1)".to_string(),
        String::new(),
    ];
    let pairs: Vec<(String, String)> = bad.iter().enumerate().map(|(i, u)| (format!("e{i}"), u.clone())).collect();
    let mut with_alias = pairs.clone();
    with_alias.push(("to-bad".into(), "alias:e0".into()));
    assert_eq!(resolve(&with_alias), Vec::new());
    for u in &bad {
        assert_eq!(crate::images::emoji_address(u), None, "{u}");
        assert_eq!(Source::Emoji(u.clone()).checked(), None, "{u}");
    }
}

#[test]
fn names_no_shortcode_may_have_are_not_read() {
    let r = resolve(&list(&[("a b", &pic("x")), ("x\u{1b}[2J", &pic("y")), ("", &pic("z")), ("ok_1+-'", &pic("w"))]));
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].0, "ok_1+-'");
}

#[test]
fn an_emoji_source_is_checked_again_by_its_kind() {
    let e = pic("x");
    assert_eq!(Source::Emoji(e.clone()).checked(), Some(e.clone()));
    assert_eq!(crate::images::host_of(&e), Some(EMOJI_HOSTS[0]));
    // An emoji's address passed off as a photo or a thumbnail, or the other way round: refused.
    assert_eq!(Source::Avatar(e.clone()).checked(), None);
    assert_eq!(Source::Thumb(e).checked(), None);
    let photo = format!("https://{}/x.png", crate::images::AVATAR_HOSTS[0]);
    assert_eq!(Source::Emoji(photo).checked(), None);
}

/// Korean names a workspace gives its own emoji (synthetic).
const KOREAN: &[&str] = &[
    "\u{C9DD}\u{C9DD}-\u{BCD1}\u{C544}\u{B9AC}",
    "\u{CD95}\u{D558}\u{CD95}\u{D558}",
    "\u{C640}\u{C6B0}-",
    "\u{C798}\u{BD80}\u{D0C1}_\u{B4DC}\u{B824}\u{C694}",
    "\u{C9F1}\u{AD6C}_\u{C6B0}\u{C640}",
    "\u{C9F1}\u{AD6C}_wow-2",
];

#[test]
fn a_name_of_any_script_s_letters_is_a_shortcode() {
    for name in KOREAN.iter().copied().chain(["+1", "it's", "party-blob", "\u{1100}\u{1161}", "caf\u{E9}"]) {
        assert!(shortcode(name), "{name:?}");
    }
}

#[test]
fn a_name_hiding_or_joining_characters_is_no_shortcode() {
    let refused = [
        "abc\u{202E}gpj",
        "a\u{200D}b",
        "a\u{200C}b",
        "a\u{200E}b",
        "a\u{2066}b\u{2069}",
        "a\u{061C}b",
        "a\u{FEFF}b",
        "a\u{E0041}b",
        "a\u{E000}",
        "a\u{F8FF}",
        "a\u{10EEEE}",
        "a\u{F0000}",
        "a b",
        "a\u{3000}b",
        "a\u{A0}b",
        "a\tb",
        "a\u{85}b",
        "a:b",
        "a\u{1b}[2J",
        "e\u{301}",
        "a\u{345}",
        "\u{915}\u{93E}",
        "\u{AC00}\u{20DD}",
        "a\u{FE0F}",
        "",
    ];
    for name in refused {
        assert!(!shortcode(name), "{name:?}");
    }
}

#[test]
fn a_shortcode_holds_at_most_64_characters_and_100_bytes() {
    assert!(shortcode(&"a".repeat(SHORTCODE_CHARS)));
    assert!(!shortcode(&"a".repeat(SHORTCODE_CHARS + 1)), "65 characters");
    // 33 syllables of three bytes each are 99 bytes; 34 are 102.
    assert!(shortcode(&"\u{AC00}".repeat(33)));
    assert!(!shortcode(&"\u{AC00}".repeat(34)), "over 100 bytes");
    assert!(shortcode(&format!("{}a", "\u{AC00}".repeat(33))), "100 bytes");
    assert!(!shortcode(&format!("{}ab", "\u{AC00}".repeat(33))), "101 bytes");
}

#[test]
fn korean_names_and_aliases_are_read_and_followed() {
    let (blob, logo) = (pic("x"), pic("y"));
    let korean_alias = "\u{CD95}\u{D558}\u{CD95}\u{D558}";
    let mut pairs: Vec<(&str, &str)> = KOREAN.iter().map(|n| (*n, blob.as_str())).collect();
    pairs.retain(|(n, _)| *n != korean_alias);
    pairs.push((korean_alias, "alias:party-blob"));
    pairs.push(("party-blob", &logo));
    pairs.push(("to-korean", "alias:\u{C9DD}\u{C9DD}-\u{BCD1}\u{C544}\u{B9AC}"));
    pairs.push(("\u{C88B}\u{C544}", "alias:white_check_mark"));
    let r = resolve(&list(&pairs));
    for name in KOREAN.iter().filter(|n| **n != korean_alias) {
        assert_eq!(get(&r, name), Some(Custom::Picture(blob.clone())), "{name}");
    }
    assert_eq!(get(&r, korean_alias), Some(Custom::Picture(logo)), "a Korean alias of an ASCII name");
    assert_eq!(get(&r, "to-korean"), Some(Custom::Picture(blob)), "an alias of a Korean name");
    assert_eq!(get(&r, "\u{C88B}\u{C544}"), Some(Custom::Standard("white_check_mark".into())));
}

#[test]
fn an_alias_to_a_name_hiding_characters_draws_nothing() {
    let r = resolve(&list(&[("a", "alias:b\u{202E}c"), ("d", "alias:e\u{200D}f"), ("g", "alias:\u{E000}")]));
    assert_eq!(r, Vec::new());
}
