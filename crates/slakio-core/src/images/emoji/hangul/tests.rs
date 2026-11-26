use super::*;
use crate::images::emoji::shortcode;

/// The NFD of "gamsahaeyong" (four syllables, two with a trailing consonant).
const THANKS_NFD: &str = "\u{1100}\u{1161}\u{11B7}\u{1109}\u{1161}\u{1112}\u{1162}\u{110B}\u{116D}\u{11BC}";
/// The same name typed (NFC).
const THANKS_NFC: &str = "\u{AC10}\u{C0AC}\u{D574}\u{C6A9}";

#[test]
fn an_nfd_name_with_a_final_consonant_composes() {
    assert_eq!(compose_hangul("\u{1100}\u{1161}\u{11B7}"), "\u{AC10}");
    assert!(shortcode(&compose_hangul("\u{1100}\u{1161}\u{11B7}")));
}

#[test]
fn an_nfd_name_without_a_final_consonant_composes() {
    assert_eq!(compose_hangul("\u{1100}\u{1161}"), "\u{AC00}");
    // The last syllable has no trailing consonant: sa-hae.
    assert_eq!(compose_hangul("\u{1109}\u{1161}\u{1112}\u{1162}"), "\u{C0AC}\u{D574}");
    assert_eq!(compose_hangul(THANKS_NFD), THANKS_NFC);
}

#[test]
fn a_name_mixing_ascii_and_nfd_hangul_composes() {
    let nfd = format!("ok_{THANKS_NFD}-2");
    assert_eq!(compose_hangul(&nfd), format!("ok_{THANKS_NFC}-2"));
    assert!(shortcode(&compose_hangul(&nfd)));
    assert!(!shortcode(&nfd), "the jamo themselves stay refused");
    let between = format!("a{}b{}c", "\u{1100}\u{1161}", "\u{1112}\u{1161}\u{11AB}");
    assert_eq!(compose_hangul(&between), "a\u{AC00}b\u{D55C}c");
}

#[test]
fn an_nfd_shortcode_in_colons_composes() {
    assert_eq!(compose_hangul(&format!(":{THANKS_NFD}:")), format!(":{THANKS_NFC}:"));
}

#[test]
fn an_nfc_name_comes_back_byte_identical() {
    for name in [THANKS_NFC, "party-blob", "ok_\u{AC10}", "caf\u{E9}", ""] {
        let got = compose_hangul(name);
        assert!(matches!(got, Cow::Borrowed(_)), "{name:?}");
        assert_eq!(got.as_bytes(), name.as_bytes());
    }
}

#[test]
fn a_precomposed_syllable_then_a_final_jamo_composes() {
    assert_eq!(compose_hangul("\u{AC00}\u{11B7}"), "\u{AC10}");
    // A syllable that has its trailing consonant already takes no second one.
    assert_eq!(compose_hangul("\u{AC10}\u{11A8}"), "\u{AC10}\u{11A8}");
}

#[test]
fn a_lone_or_archaic_jamo_stays_and_stays_refused() {
    for name in [
        "\u{1100}",
        "\u{1161}",
        "\u{11A8}",
        "\u{1161}\u{1100}",
        "\u{1113}\u{1161}",
        "\u{1100}\u{1176}",
        "\u{11FF}",
        "\u{1100}\u{1161}\u{11C3}",
    ] {
        let got = compose_hangul(name);
        assert!(!shortcode(&got), "{name:?} -> {got:?}");
    }
    assert_eq!(compose_hangul("\u{1113}\u{1161}"), "\u{1113}\u{1161}");
    assert_eq!(compose_hangul("\u{1100}\u{1161}\u{11C3}"), "\u{AC00}\u{11C3}");
}

#[test]
fn latin_nfd_is_left_as_it_is() {
    let got = compose_hangul("cafe\u{301}");
    assert_eq!(got, "cafe\u{301}");
    assert!(!shortcode(&got));
    assert_eq!(compose_hangul("\u{3131}\u{314F}"), "\u{3131}\u{314F}", "compatibility jamo");
}
