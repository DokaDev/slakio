// Korean text is written with `\u{…}` escapes (no Hangul in source files).

use super::*;

const HELLO: &str = "\u{C548}\u{B155}\u{D558}\u{C138}\u{C694}";

fn typed(s: &str) -> Composer {
    let mut c = Composer::default();
    for ch in s.chars() {
        c.insert(ch.encode_utf8(&mut [0; 4]));
    }
    c
}

#[test]
fn ime_commits_and_typing_build_the_text_and_backspace_takes_whole_graphemes() {
    let mut c = typed(HELLO);
    c.insert(" \u{1F469}\u{200D}\u{1F4BB}");
    assert_eq!(c.text(), format!("{HELLO} \u{1F469}\u{200D}\u{1F4BB}"));
    assert!(c.backspace());
    assert_eq!(c.text(), format!("{HELLO} "), "the emoji sequence goes as one");
    c.left();
    c.left();
    assert!(c.backspace());
    assert_eq!(c.text(), "\u{C548}\u{B155}\u{D558}\u{C694} ");
    assert!(c.delete());
    assert_eq!(c.text(), "\u{C548}\u{B155}\u{D558} ");
}

#[test]
fn pasted_escape_sequences_and_bidi_controls_never_get_in() {
    let mut c = Composer::default();
    c.insert("a\x1b]52;c;ZXZpbA==\x07b\u{202E}c\r\nd");
    assert_eq!(c.text(), "abc\nd");
}

#[test]
fn ctrl_w_deletes_the_word_before_the_cursor() {
    let mut c = typed("deploy the  build");
    assert!(c.delete_word_back());
    assert_eq!(c.text(), "deploy the  ");
    assert!(c.delete_word_back());
    assert_eq!(c.text(), "deploy ");
    let mut c = typed("path/to/file");
    c.delete_word_back();
    assert_eq!(c.text(), "path/to/");
    c.delete_word_back();
    assert_eq!(c.text(), "path/to");
    let mut k = typed(&format!("x {HELLO}"));
    k.delete_word_back();
    assert_eq!(k.text(), "x ", "a Korean word is a word");
    let mut lines = typed("one\n");
    lines.delete_word_back();
    assert_eq!(lines.text(), "one", "at the start of a line it joins the line above");
    assert!(!Composer::default().delete_word_back());
}

#[test]
fn lines_move_and_take_empties_it() {
    let mut c = typed("first\nsecond line");
    c.vertical(-1);
    assert_eq!(c.cursor(), "first".len(), "the column is kept where the line is long enough");
    c.home();
    assert_eq!(c.cursor(), 0);
    c.vertical(1);
    c.end();
    assert_eq!(c.cursor(), c.text().len());
    assert!(c.delete_line_back());
    assert_eq!(c.text(), "first\n");
    assert_eq!(c.take(), "first\n");
    assert!(c.is_empty() && c.cursor() == 0);
}

#[test]
fn the_view_wraps_by_cell_width_and_places_the_cursor() {
    let c = typed("abcdef");
    assert_eq!(c.view(4), View { lines: vec!["abcd".into(), "ef".into()], cursor: (1, 2) });
    let c = typed("abcd");
    assert_eq!(c.view(4).cursor, (1, 0), "a full line puts the cursor on the next");
    let wide = typed(&HELLO[..9]);
    assert_eq!(wide.view(5).lines, ["\u{C548}\u{B155}", "\u{D558}"], "a wide character never splits");
    let mut c = typed("ab\ncd");
    c.home();
    assert_eq!(c.view(10), View { lines: vec!["ab".into(), "cd".into()], cursor: (1, 0) });
    let c = typed("ab\n");
    assert_eq!(c.view(10), View { lines: vec!["ab".into(), String::new()], cursor: (1, 0) });
    assert_eq!(Composer::default().view(10), View { lines: vec![String::new()], cursor: (0, 0) });
}
