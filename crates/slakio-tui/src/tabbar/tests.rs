use super::*;

fn label(n: usize, title: &str, badge: Option<&str>) -> Label {
    Label { number: n.to_string(), title: title.to_string(), badge: badge.map(str::to_string) }
}

/// The bar as text, the marks in their columns.
fn text(b: &Bar) -> String {
    let mut row = vec![' '; usize::from(b.area.width)];
    let mut put = |x: u16, s: &str| {
        let mut at = usize::from(x - b.area.x);
        for c in s.chars() {
            row[at] = c;
            at += 1;
        }
    };
    for p in &b.pieces {
        put(p.x, &p.text);
    }
    if let Some((x, _)) = b.left {
        put(x, MORE_LEFT);
    }
    if let Some((x, _)) = b.right {
        put(x, MORE_RIGHT);
    }
    row.into_iter().collect::<String>().trim_end().to_string()
}

fn area(w: u16) -> Rect {
    Rect::new(5, 2, w, 1)
}

#[test]
fn tabs_that_fit_are_drawn_whole_and_every_column_leads_where_it_shows() {
    let labels = [label(1, "#backend", None), label(2, "⤷ Deploy rollback", Some("●3"))];
    let b = layout(area(80), &labels, 0);
    assert_eq!(text(&b), " 1 #backend ×  2 ⤷ Deploy rollback ●3 ×");
    assert!(b.left.is_none() && b.right.is_none());
    let close = |i: usize| b.pieces.iter().find(|p| p.tab == i && p.part == Part::Close).unwrap().x;
    assert_eq!(b.hit(close(0)), Some(Hit::Close(0)));
    assert_eq!(b.hit(close(1)), Some(Hit::Close(1)));
    assert_eq!(b.hit(close(0) - 1), Some(Hit::Tab(0)), "the space before × is the tab's");
    assert_eq!(b.hit(close(0) + 1), Some(Hit::Tab(0)), "and the space after it");
    assert_eq!(b.hit(close(0) + 2), Some(Hit::Tab(1)), "the next tab starts right after");
    assert_eq!(b.hit(5), Some(Hit::Tab(0)));
    assert_eq!(b.hit(5 + 79), None, "past the last tab");
    assert_eq!(b.tab_at(close(1)), Some(1));
}

#[test]
fn titles_shorten_before_tabs_are_left_out() {
    let long = "a-channel-with-a-very-long-name";
    let labels: Vec<Label> = (1..=3).map(|n| label(n, long, None)).collect();
    assert!(text(&layout(area(120), &labels, 0)).contains("very-long-name ×"), "whole where they fit");
    let b = layout(area(100), &labels, 0);
    assert!(b.left.is_none() && b.right.is_none(), "{}", text(&b));
    assert!(text(&b).contains("a-channel-with-a-very-l… ×"), "{}", text(&b));
    assert!(text(&layout(area(80), &labels, 0)).contains("a-channel-with-… ×"), "a step shorter");
    let b = layout(area(50), &labels, 0);
    assert!(b.right.is_none(), "shorter still, all fit: {}", text(&b));
    assert!(width(&text(&b)) <= 50);
}

#[test]
fn tabs_that_do_not_fit_scroll_to_keep_the_one_shown_with_marks_at_the_ends() {
    let labels: Vec<Label> = (1..=12).map(|n| label(n, &format!("#channel-{n}"), None)).collect();
    let b = layout(area(60), &labels, 6);
    let t = text(&b);
    assert!(b.pieces.iter().any(|p| p.tab == 6), "the tab shown stays in view: {t}");
    let (lx, li) = b.left.expect("tabs left out on the left");
    let (rx, ri) = b.right.expect("and on the right");
    assert_eq!((lx, rx), (5, 5 + 59));
    assert!(t.starts_with('‹') && t.ends_with('›'), "{t}");
    assert_eq!(b.hit(lx), Some(Hit::More(li)));
    assert_eq!(b.hit(rx), Some(Hit::More(ri)));
    let first = b.pieces.first().unwrap().tab;
    assert_eq!(li + 1, first, "the nearest one left out");
    assert_eq!(ri, b.pieces.last().unwrap().tab + 1);
    assert!(b.pieces.iter().all(|p| p.x > lx && p.x < rx), "nothing drawn over the marks");
    // The first tab shown: no mark on the left.
    let b = layout(area(60), &labels, 0);
    assert!(b.left.is_none() && b.right.is_some());
}

#[test]
fn wide_titles_keep_their_columns() {
    let hangul = "\u{C7A5}\u{C560}\u{B300}\u{C751}";
    let labels = [label(1, hangul, Some("●1")), label(2, "#ops", None)];
    let b = layout(area(40), &labels, 1);
    let title = b.pieces.iter().find(|p| p.tab == 0 && p.part == Part::Title).unwrap();
    let badge = b.pieces.iter().find(|p| p.tab == 0 && p.part == Part::Badge).unwrap();
    assert_eq!(badge.x, title.x + 8, "four syllables are eight columns");
    let ops = b.pieces.iter().find(|p| p.tab == 1 && p.part == Part::Number).unwrap();
    assert_eq!(b.hit(ops.x), Some(Hit::Tab(1)));
    assert_eq!(b.hit(title.x + 7), Some(Hit::Tab(0)), "the second cell of a wide character");
}

#[test]
fn an_empty_bar_leads_nowhere() {
    let b = layout(area(30), &[], 0);
    assert!(b.pieces.is_empty() && b.hit(5).is_none());
}
