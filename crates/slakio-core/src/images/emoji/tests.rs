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
