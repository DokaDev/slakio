use super::*;
use ratatui::layout::Rect;
use slakio_core::model::{ConversationId, Ts, UserId};
use slakio_core::sanitize::Remote;

fn hit(text: &str) -> Hit {
    Hit {
        conversation: ConversationId::new("C1"),
        name: Remote::new("general"),
        ts: Ts(1),
        thread: None,
        user: UserId::new("U1"),
        author: Remote::new("alex"),
        text: Remote::new(text),
        permalink: String::new(),
    }
}

#[test]
fn the_marks_split_a_text_into_plain_and_found_runs() {
    let got = runs(&hit("the \u{E000}deploy\u{E001} went \u{E000}out\u{E001}"));
    let want = [("the ", false), ("deploy", true), (" went ", false), ("out", true)];
    assert_eq!(got, want.map(|(s, m)| (s.to_string(), m)));
    assert_eq!(runs(&hit("plain")), [("plain".to_string(), false)]);
    assert!(runs(&hit("")).is_empty());
    // Marks out of order or left open never lose text.
    let odd = runs(&hit("\u{E001}a\u{E000}b\u{E000}\u{E001}c\u{E000}"));
    assert_eq!(odd.iter().map(|(s, _)| s.as_str()).collect::<String>(), "abc");
}

#[test]
fn the_popup_fits_the_screen_and_keeps_the_cursor_in_view() {
    for (w, h) in [(120, 40), (80, 24), (50, 24), (20, 10)] {
        let size = Rect::new(0, 0, w, h);
        let b = search_box(size, 0).expect("fits");
        assert!(b.rect.right() <= w && b.rect.bottom() < h, "{w}x{h}: {b:?}");
        assert!(b.shown() >= 1 && b.shown() <= usize::from(HITS_MAX));
        let far = search_box(size, 30).unwrap();
        assert!((far.first..far.first + far.shown()).contains(&30));
    }
    assert_eq!(search_box(Rect::new(0, 0, 19, 40), 0), None);
    assert_eq!(search_box(Rect::new(0, 0, 80, 9), 0), None);
}

#[test]
fn a_long_text_keeps_a_bounded_number_of_runs_of_bounded_length() {
    let many = "\u{E000}a\u{E001}b".repeat(10_000);
    let got = runs(&hit(&many));
    assert_eq!(got.len(), RUNS_MAX);
    let lead = format!("{}x \u{E000}deploy\u{E001} {}", "w".repeat(RUN_CHARS_MAX * 3), "z".repeat(RUN_CHARS_MAX * 3));
    let got = runs(&hit(&lead));
    assert_eq!(got.len(), 3);
    assert_eq!(got[0].0.chars().count(), RUN_CHARS_MAX);
    assert!(got[0].0.ends_with("x "), "the text before the word found keeps its end");
    assert_eq!(got[1], ("deploy".to_string(), true));
    assert!(got[2].0.starts_with(" z") && got[2].0.chars().count() == RUN_CHARS_MAX);
}
