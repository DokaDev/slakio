//! The flow tests' terminal emulator (`support/pty.rs`): what it leaves off the screen (the
//! strings a terminal takes in without drawing them, up to `ESC \`, however the typescript was
//! cut while it was being written) and the foreground color it keeps as a picture's id.

#[path = "support/pty.rs"]
mod pty;

use pty::{cells, screen, sgr};

/// The first row of the screen after `t`, its trailing blanks left out.
fn row(t: &str) -> String {
    screen(t, 2, 20).lines().next().unwrap().trim_end().to_string()
}

#[test]
fn a_string_up_to_its_terminator_is_never_drawn() {
    for intro in ['_', 'P', '^', 'X'] {
        let t = format!("ab\u{1b}{intro}Ga=T,f=100;AAAA\u{1b}\\cd");
        assert_eq!(row(&t), "abcd", "ESC {intro}");
    }
}

#[test]
fn a_stray_escape_inside_a_string_does_not_end_it() {
    assert_eq!(row("ab\u{1b}_Gm=1;xx\u{1b}yy\u{1b}\u{1b}\\cd"), "abcd");
    assert_eq!(row("ab\u{1b}Pq\u{1b}[31mzz\u{1b}\\cd"), "abcd", "a CSI inside is the string's too");
}

#[test]
fn a_picture_cut_while_written_draws_nothing_and_the_rest_lands_once_written() {
    let whole = "ab\u{1b}_Ga=T,f=100,m=1;AAAABBBB\u{1b}\\cd";
    for cut in 0..=whole.len() {
        let Some(head) = whole.get(..cut) else { continue };
        let shown = row(head);
        let want = match cut {
            0..=2 => head.to_string(),
            _ if cut < whole.len() - 2 => "ab".to_string(),
            _ => head.replace(&whole[2..whole.len() - 2], ""),
        };
        assert_eq!(shown, want, "cut at {cut}");
    }
    // The tail unfinished: what came before is kept, none of it drawn.
    assert_eq!(row("ab\u{1b}_Ga=T;AAAA\u{1b}"), "ab");
    // Two chunks written one after the other read as the whole.
    let (first, second) = whole.split_at(whole.find("BB").unwrap());
    assert_eq!(row(&format!("{first}{second}")), "abcd");
}

#[test]
fn an_osc_ends_at_its_bell() {
    assert_eq!(row("ab\u{1b}]0;title\u{7}cd"), "abcd");
}

#[test]
fn the_foreground_color_is_an_id_only_while_it_is_24_bit() {
    assert_eq!(sgr("38;2;0;1;2", None), Some(0x00_01_02));
    assert_eq!(sgr("1;38;2;0;0;7", None), Some(7), "after a bold");
    assert_eq!(sgr("48;2;9;9;9", Some(7)), Some(7), "a background keeps it");
    assert_eq!(sgr("48;5;3", Some(7)), Some(7));
    assert_eq!(sgr("", Some(7)), None, "a reset");
    assert_eq!(sgr("0", Some(7)), None);
    assert_eq!(sgr("39", Some(7)), None);
    assert_eq!(sgr("31", Some(7)), None, "another color");
    assert_eq!(sgr("91", Some(7)), None);
    assert_eq!(sgr("38;5;12", Some(7)), None);
    assert_eq!(sgr("38;2;0;0;7;48;2;1;1;1", None), Some(7), "the background after it skipped whole");
}

#[test]
fn a_placeholder_carries_its_color_and_other_text_none() {
    let t = "\u{1b}[38;2;0;0;5m\u{10EEEE}x\u{1b}[0m\u{10EEEE}";
    let grid = cells(t, 1, 4);
    assert_eq!(grid[0][0], ('\u{10EEEE}', Some(5)));
    assert_eq!(grid[0][1], ('x', None));
    assert_eq!(grid[0][2], ('\u{10EEEE}', None));
}
