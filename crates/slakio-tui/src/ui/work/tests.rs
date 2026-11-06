use super::fit;
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
