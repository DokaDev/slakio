use super::*;
use unicode_width::UnicodeWidthStr;

#[test]
fn a_placeholder_is_one_cell_wide_and_one_grapheme() {
    use unicode_segmentation::UnicodeSegmentation;
    for (r, c) in [(0, 0), (3, 7), (MARKS - 1, MARKS - 1)] {
        let p = cell(r, c);
        assert_eq!(p.width(), 1, "{r},{c}");
        assert_eq!(p.graphemes(true).count(), 1, "{r},{c}");
    }
    let line = row(2, 5);
    assert_eq!(line.width(), 5);
    assert_eq!(line.graphemes(true).count(), 5);
}

#[test]
fn a_picture_is_sent_in_pieces_and_never_asks_for_an_answer() {
    let png = vec![7u8; 10_000];
    let out = transmit(42, 12, 5, &png);
    let commands: Vec<&str> = out.split("\x1b\\").filter(|c| !c.is_empty()).collect();
    assert!(commands.len() >= 4, "{} pieces", commands.len());
    assert!(commands[0].starts_with("\x1b_Ga=T,U=1,f=100,t=d,i=42,c=12,r=5,q=2,m=1;"));
    for c in &commands {
        assert!(c.starts_with("\x1b_G") && c.contains("q=2"), "{c:.40}");
        let data = c.split_once(';').unwrap().1;
        assert!(data.len() <= 4096);
    }
    assert!(commands.last().unwrap().contains("m=0;"));
    assert_eq!(delete(42), "\x1b_Ga=d,d=I,i=42,q=2\x1b\\");
}

#[test]
fn the_colour_says_the_id() {
    assert_eq!(color(0x01_02_03), Color::Rgb(1, 2, 3));
    assert_eq!(color(ID_MAX), Color::Rgb(255, 255, 255));
}
