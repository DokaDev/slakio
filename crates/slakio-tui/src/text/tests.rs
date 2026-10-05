use super::*;

#[test]
fn widths_count_wide_and_zero_width_graphemes() {
    assert_eq!(width("abc"), 3);
    assert_eq!(width("\u{D55C}\u{AE00}"), 4);
    assert_eq!(width("\u{1F469}\u{200D}\u{1F4BB}"), 2);
    assert_eq!(width("e\u{0301}"), 1);
}

#[test]
fn clip_ends_with_an_ellipsis_and_never_splits_a_wide_character() {
    assert_eq!(clip("hello", 10), "hello");
    assert_eq!(clip("hello world", 6), "hello…");
    assert_eq!(clip("\u{D55C}\u{AE00}\u{D55C}", 4), "\u{D55C}…");
    assert_eq!(clip("abc", 0), "");
}

#[test]
fn wrap_keeps_words_and_line_breaks() {
    assert_eq!(wrap("one two three", 7, 9), (vec!["one two".into(), "three".into()], false));
    assert_eq!(wrap("a\nb", 10, 9), (vec!["a".into(), "b".into()], false));
    assert_eq!(wrap("", 10, 9), (vec![String::new()], false));
    let (lines, more) = wrap(&"x".repeat(25), 10, 9);
    assert_eq!(lines, ["x".repeat(10), "x".repeat(10), "x".repeat(5)]);
    assert!(!more);
}

#[test]
fn wrap_stops_at_the_line_limit_and_says_so() {
    let (lines, more) = wrap(&"word ".repeat(100), 10, 3);
    assert_eq!(lines.len(), 3);
    assert!(more);
    let (lines, more) = wrap("a\nb\nc\nd", 10, 2);
    assert_eq!((lines.len(), more), (2, true));
    let (lines, more) = wrap("a\nb", 10, 2);
    assert_eq!((lines.len(), more), (2, false));
}

#[test]
fn wrapped_lines_never_exceed_the_width() {
    let text = "\u{D55C}\u{AE00} wide \u{1F469}\u{200D}\u{1F4BB} text, with a verylongwordthatbreaks and more";
    for w in 2..30 {
        let (lines, _) = wrap(text, w, 100);
        for l in &lines {
            assert!(width(l) <= w.max(2), "{w}: {l:?}");
        }
    }
}

#[test]
fn punctuation_never_starts_a_line_on_its_own() {
    // The period stays with its word, also where that word must move down.
    let trimmed = |t: &str, w: usize| wrap(t, w, 9).0.iter().map(|l| l.trim_end().to_string()).collect::<Vec<_>>();
    assert_eq!(trimmed("quitting loses it.", 17), ["quitting loses", "it."]);
    assert_eq!(trimmed("abc def.", 7), ["abc", "def."]);
    for (text, w) in [("one, two, three!", 9), ("(see above).", 6), ("\u{C548}\u{B155}\u{D558}\u{C138}\u{C694}.", 10)] {
        let (lines, _) = wrap(text, w, 9);
        for l in &lines {
            assert!(!l.starts_with(['.', ',', '!', ')']), "{text:?} at {w}: {lines:?}");
            assert!(width(l) <= w, "{text:?} at {w}: {lines:?}");
        }
    }
}
