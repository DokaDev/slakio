// Property tests run on the stable toolchain (proptest); the cargo-fuzz target in `fuzz/` checks
// the same invariants on a nightly schedule. Every input a fuzz run finds becomes a case here.

use super::*;
use proptest::prelude::*;

fn line(s: &str) -> String {
    sanitize_line(s).into_string()
}

fn block(s: &str) -> String {
    sanitize_block(s).into_string()
}

/// What may never reach the screen.
fn forbidden(c: char, block: bool) -> bool {
    (is_control(c) && (c != '\n' || !block)) || is_invisible(c) || is_unsafe_glyph(c)
}

/// The invariants of sanitised text (`block` keeps line breaks).
fn check(out: &str, block: bool) {
    let max = if block { BLOCK_MAX_CHARS } else { LINE_MAX_CHARS };
    assert!(out.chars().count() <= max, "too long: {}", out.chars().count());
    let chars: Vec<char> = out.chars().collect();
    let mut marks = 0;
    for (k, &c) in chars.iter().enumerate() {
        assert!(!forbidden(c, block), "{c:?} at {k} in {out:?}");
        if c == '\u{200D}' {
            let prev = k.checked_sub(1).map(|p| chars[p]);
            let next = chars.get(k + 1).copied();
            assert!(prev.is_some_and(is_emoji) && next.is_some_and(is_pictograph), "a lone joiner in {out:?}");
        }
        marks = if is_mark(c) { marks + 1 } else { 0 };
        assert!(marks <= MAX_MARKS, "a mark flood in {out:?}");
    }
}

#[test]
fn plain_text_is_kept_as_it_is() {
    for s in ["", "hello", "Starting deploy :tada:", "a  b", "\u{D55C}\u{AE00} \u{1F469}\u{200D}\u{1F4BB}", "caf\u{E9}"]
    {
        assert_eq!(line(s), s);
        assert_eq!(block(s), s);
    }
    assert_eq!(block("one\ntwo"), "one\ntwo");
}

#[test]
fn escape_sequences_go_whole_and_the_text_around_them_stays() {
    let cases = [
        ("before\x1b[2J\x1b[Hafter", "beforeafter"),
        ("\x1b[31;1mred\x1b[0m", "red"),
        ("\x1b[?1049l\x1b[?25lx", "x"),
        ("\x1b]0;owned\x07visible", "visible"),
        ("\x1b]52;c;Y3VybA==\x1b\\", ""),
        ("\x1b]8;;https://evil.example.invalid/\x1b\\docs\x1b]8;;\x1b\\", "docs"),
        ("\x1bP+q544e\x1b\\after", "after"),
        ("\x1b_Ga=T;AAAA\x1b\\after", "after"),
        ("\x1bX sos \x1b\\a", "a"),
        ("\x1b^ pm \x1b\\a", "a"),
        ("\u{9b}31mc1\u{9b}0m", "c1"),
        ("\u{9d}0;title\u{9c}text", "text"),
        ("\u{90}dcs\u{9c}text", "text"),
        ("x\x1b(By", "xy"),
        ("x\x1bcy", "xy"),
        ("x\x1b7y\x1b8", "xy"),
        ("dangling\x1b", "dangling"),
    ];
    for (input, want) in cases {
        assert_eq!(line(input), want, "{input:?}");
        assert_eq!(block(input), want, "{input:?}");
    }
}

#[test]
fn an_unterminated_string_sequence_loses_only_its_introducer() {
    // A terminal would swallow the rest; here the reader sees it, harmless.
    assert_eq!(line("\x1b]0;title without end"), "0;title without end");
    assert_eq!(line("\x1b[12"), "12");
    assert_eq!(line("\x1b[1;2\u{D55C}"), "1;2\u{D55C}");
}

#[test]
fn control_characters_are_marked_and_line_breaks_kept_in_blocks_only() {
    assert_eq!(line("a\x00b\x07c\x08d\x7fe"), "a\u{FFFD}b\u{FFFD}c\u{FFFD}d\u{FFFD}e");
    assert_eq!(line("innocent\rEVIL"), "innocent EVIL");
    assert_eq!(block("innocent\rEVIL"), "innocent\nEVIL");
    assert_eq!(block("a\r\nb\u{2028}c\u{2029}d\u{85}e"), "a\nb\nc\nd\ne");
    assert_eq!(line("a\r\nb"), "a b");
    assert_eq!(block("c1\tc2"), "c1    c2");
    assert_eq!(line("c1\tc2"), "c1 c2");
    assert_eq!(line("\u{80}\u{9c}"), "\u{FFFD}\u{FFFD}");
}

#[test]
fn bidi_and_invisible_characters_go() {
    assert_eq!(line("invoice_\u{202E}fdp.exe\u{202C}"), "invoice_fdp.exe");
    assert_eq!(line("\u{2067}\u{2066}\u{200F}\u{200E}open"), "open");
    assert_eq!(line(&format!("a{}b", "\u{200B}".repeat(1000))), "ab");
    assert_eq!(line("x\u{200D}\u{200C}\u{FEFF}\u{2060}y"), "xy");
    assert_eq!(line("\u{E0041}\u{E0042}\u{E007F}"), "");
}

#[test]
fn joiners_stay_inside_emoji_and_one_selector_after_a_character() {
    let tech = "\u{1F469}\u{200D}\u{1F4BB}";
    assert_eq!(line(tech), tech);
    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
    assert_eq!(line(family), family);
    assert_eq!(line("\u{2764}\u{FE0F}\u{FE0E}"), "\u{2764}\u{FE0F}");
    assert_eq!(line("\u{FE0F}x"), "x", "nothing to select");
    assert_eq!(line("\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}"), "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}");
}

#[test]
fn private_use_and_noncharacters_cannot_pass_for_icons() {
    assert_eq!(line("\u{E000}\u{F8FF}\u{F02DC}\u{10FFFF}\u{FFFE}\u{FDD0}"), "\u{FFFD}".repeat(6));
}

#[test]
fn combining_mark_floods_are_cut_to_two_marks() {
    let flood = format!("e{}", "\u{0301}\u{0336}".repeat(250));
    assert_eq!(line(&flood), "e\u{0301}\u{0336}");
    assert_eq!(line("e\u{0301}a\u{0301}"), "e\u{0301}a\u{0301}", "two characters, one mark each");
}

#[test]
fn long_text_is_cut_with_an_ellipsis() {
    let long = "x".repeat(100 * 1024);
    let l = line(&long);
    assert_eq!(l.chars().count(), LINE_MAX_CHARS);
    assert!(l.ends_with('…'));
    let b = block(&long);
    assert_eq!(b.chars().count(), BLOCK_MAX_CHARS);
    assert_eq!(block(&b), b, "cut text stays as it is");
    // Exactly the cap is not cut.
    let exact = "y".repeat(LINE_MAX_CHARS);
    assert_eq!(line(&exact), exact);
}

#[test]
fn remote_text_is_reached_only_through_the_sanitiser() {
    let r = Remote::new("a\x1b[2Jb\nc");
    assert_eq!(r.line().as_str(), "ab c");
    assert_eq!(r.block().as_str(), "ab\nc");
    assert_eq!(r.unsanitized(), "a\x1b[2Jb\nc");
    assert!(format!("{r:?}").contains("\\u{1b}"), "Debug escapes it: {r:?}");
    assert_eq!(Remote::from("x"), "x");
}

/// Characters that make escape sequences, controls and the other hard cases likely.
fn hostile_char() -> impl Strategy<Value = char> {
    prop_oneof![
        3 => prop::sample::select(vec![
            '\x1b', '[', ']', 'P', '_', '^', 'X', '\\', ';', '?', '0', '1', '2', 'm', 'H', 'J', '\x07',
            '\u{9b}', '\u{9c}', '\u{9d}', '\u{90}', '\u{9f}', '\r', '\n', '\t', '\0', '\x7f', '\u{85}',
            '\u{202E}', '\u{2066}', '\u{200B}', '\u{200D}', '\u{FEFF}', '\u{0301}', '\u{FE0F}', '\u{FE0E}',
            '\u{E0041}', '\u{E000}', '\u{FFFE}', '\u{2028}', '\u{1F469}', '\u{1F4BB}', '\u{2764}', '\u{D55C}', 'a', ' ',
        ]),
        1 => any::<char>(),
    ]
}

fn hostile_text() -> impl Strategy<Value = String> {
    prop::collection::vec(hostile_char(), 0..300).prop_map(|v| v.into_iter().collect())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    #[test]
    fn sanitised_text_holds_the_invariants(s in hostile_text()) {
        check(&line(&s), false);
        check(&block(&s), true);
    }

    #[test]
    fn sanitising_twice_changes_nothing(s in hostile_text()) {
        let l = line(&s);
        prop_assert_eq!(line(&l), l.clone());
        let b = block(&s);
        prop_assert_eq!(block(&b), b);
    }

    #[test]
    fn printable_text_around_a_sequence_survives(
        a in "[a-zA-Z0-9 .,!]{0,40}",
        b in "[a-zA-Z0-9 .,!]{0,40}",
        seq in prop::sample::select(vec![
            "\x1b[2J", "\x1b[38;2;1;2;3m", "\x1b]0;t\x07", "\x1b]52;c;QUFB\x1b\\", "\x1bP1$r\x1b\\",
            "\x1b_Gf=1\x1b\\", "\u{9b}1m", "\u{9d}2;x\u{9c}", "\x1b(0", "\x1bc",
        ]),
    ) {
        let input = format!("{a}{seq}{b}");
        prop_assert_eq!(line(&input), format!("{a}{b}"));
    }

}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn long_lines_respect_the_caps(s in prop::collection::vec(hostile_char(), 200..2000)) {
        let s: String = s.into_iter().collect();
        check(&line(&s), false);
    }
}
