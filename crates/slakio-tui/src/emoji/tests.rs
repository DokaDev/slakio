use super::*;
use crate::text::width;

#[test]
fn standard_shortcodes_and_slack_aliases_become_emoji() {
    assert_eq!(get("+1"), Some("\u{1F44D}"));
    assert_eq!(get("thumbsup"), Some("\u{1F44D}"));
    assert_eq!(get("white_check_mark"), Some("\u{2705}"));
    assert_eq!(get("simple_smile"), get("slightly_smiling_face"));
    assert_eq!(get("eyes"), Some("\u{1F440}"));
    assert_eq!(get("our-team-logo"), None, "a custom emoji stays text");
}

#[test]
fn skin_tones_follow_slacks_spelling() {
    assert_eq!(get("+1::skin-tone-3"), Some("\u{1F44D}\u{1F3FC}"));
    assert_eq!(replace("ok :+1::skin-tone-6: done"), "ok \u{1F44D}\u{1F3FF} done");
    assert_eq!(get("eyes::skin-tone-2"), get("eyes"), "no toned variant: the emoji itself");
}

#[test]
fn text_keeps_what_is_not_an_emoji() {
    assert_eq!(replace("deploy :rocket: at 10:30, :custom_logo: ::"), "deploy \u{1F680} at 10:30, :custom_logo: ::");
    assert_eq!(replace("no colons"), "no colons");
    assert_eq!(replace(":::fire::"), "::\u{1F525}:");
    assert_eq!(replace(":"), ":");
}

#[test]
fn emoji_are_two_cells_wide_as_one_grapheme() {
    for name in ["fire", "+1::skin-tone-4", "family_man_woman_girl_boy", "heart", "white_check_mark", "rainbow_flag"] {
        let e = get(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(width(e), 2, "{name}: {e:?}");
    }
    let s = crate::text::wrap(&replace(":fire: :fire: :fire:"), 5, 9).0;
    assert!(s.iter().all(|l| width(l) <= 5), "{s:?}");
}
