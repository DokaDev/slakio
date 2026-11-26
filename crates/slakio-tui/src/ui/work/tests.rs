use super::{PHOTO_MIN_WIDTH, fit, peer_chip};
use crate::images::graphics;
use ratatui::text::Span;

fn text(spans: &[Span<'_>]) -> String {
    spans.iter().map(|s| s.content.as_ref()).collect()
}

#[test]
fn a_title_keeps_a_blank_before_the_border_or_ends_in_an_ellipsis() {
    let title = || vec![Span::raw(" "), Span::raw("⤷ Thread · #long-threads ")];
    // Whole, with its blank.
    assert_eq!(text(&fit(title(), 26)), " ⤷ Thread · #long-threads ");
    // One cell short of the blank: the words would touch the dashes, so they are cut.
    assert_eq!(text(&fit(title(), 25)), " ⤷ Thread · #long-threa…");
    assert_eq!(text(&fit(title(), 20)), " ⤷ Thread · #long-t…");
    // What comes after a cut span is left out.
    let more = vec![Span::raw(" "), Span::raw("#backend "), Span::raw("[zoomed] ")];
    assert_eq!(text(&fit(more, 8)), " #backe…");
}

#[test]
fn a_dm_title_holds_the_whole_photo_or_falls_back_to_initials() {
    let photo = || Some(Span::raw(graphics::row(0, 2)));
    let initials = || Some(Span::raw("MK"));
    // Wide enough: the photo, and the cut (two cells of the border kept) leaves its cells whole.
    for w in [PHOTO_MIN_WIDTH, 24, 120] {
        let chip = peer_chip(w, photo, initials).unwrap();
        assert_eq!(chip.content, graphics::row(0, 2), "width {w}");
        let title = vec![Span::raw(" "), Span::raw("▌"), chip, Span::raw(" "), Span::raw("@Minsu Kim ")];
        let kept = fit(title, usize::from(w) - 4);
        assert_eq!(kept[2].content, graphics::row(0, 2), "width {w}: the photo whole");
    }
    // Narrower: the initials, and the photo is not even asked for.
    for w in [0, 4, PHOTO_MIN_WIDTH - 1] {
        let chip = peer_chip(w, || panic!("width {w}: asked for the photo"), initials);
        assert_eq!(chip.unwrap().content, "MK", "width {w}");
    }
    // No photo (off, not here yet or failed): the initials; avatars off: nothing.
    assert_eq!(peer_chip(80, || None, initials).unwrap().content, "MK");
    assert!(peer_chip(80, || None, || None).is_none());
}
