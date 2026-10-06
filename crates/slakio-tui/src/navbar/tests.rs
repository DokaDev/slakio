use super::*;

const ICONS: [&str; 5] = ["\u{F02DC}", "\u{F0361}", "\u{F009A}", "\u{F0219}", "\u{F00C0}"];

fn items(icons: bool) -> Vec<Item> {
    let names = ["Home", "DMs", "Activity", "Files", "Later"];
    let badges = [Some("@24"), Some("●11"), Some("@37"), None, None];
    (0..5)
        .map(|i| Item {
            glyph: icons.then_some(ICONS[i]),
            label: names[i].to_string(),
            badge: badges[i].map(str::to_string),
        })
        .collect()
}

fn ws() -> Chip {
    Chip { name: "A company".to_string(), others: vec![("B".to_string(), "@2".to_string())] }
}

/// A line as text; a glyph slot shows as `G_` (its two cells).
fn text(b: &Bar) -> String {
    let mut s = String::new();
    for p in &b.pieces {
        s.push_str(if p.part == Part::Glyph { "G_" } else { &p.text });
    }
    s
}

fn rows(w: u16, icons: bool, shown: usize, folded: bool) -> Vec<String> {
    views(Rect::new(0, 1, w, 5), &items(icons), shown, folded).iter().map(text).collect()
}

#[test]
fn a_row_per_view_its_count_at_the_right() {
    assert_eq!(
        rows(28, true, 0, false),
        [
            " G_Home                 @24 ",
            " G_DMs                  ●11 ",
            " G_Activity             @37 ",
            " G_Files                    ",
            " G_Later                    ",
        ]
    );
    assert_eq!(rows(28, false, 2, false)[2], " Activity               @37 ", "icons off: the names alone");
    let b = views(Rect::new(3, 1, 28, 5), &items(true), 0, false);
    assert_eq!(b.iter().map(|r| r.area.y).collect::<Vec<_>>(), [1, 2, 3, 4, 5], "one row each, top down");
}

#[test]
fn folded_the_view_shown_alone_after_a_fold_mark() {
    assert_eq!(rows(28, true, 2, true), [" ▸ G_Activity           @37 "]);
    assert_eq!(rows(28, false, 1, true), [" ▸ DMs                  ●11 "]);
    let b = views(Rect::new(3, 1, 28, 1), &items(true), 2, true);
    assert_eq!((b.len(), b[0].hit(3), b[0].hit(30)), (1, Some(2), Some(2)), "the whole row is the view shown's");
}

#[test]
fn a_count_is_never_cut_the_name_is() {
    for w in 14..60 {
        for icons in [false, true] {
            for folded in [false, true] {
                for shown in 0..5 {
                    for (i, b) in views(Rect::new(3, 1, w, 5), &items(icons), shown, folded).iter().enumerate() {
                        let used: u16 = b.pieces.iter().map(|p| p.width).sum();
                        assert_eq!(used, w, "{w} {icons}: a row is the panel's width");
                        assert!(b.pieces.windows(2).all(|p| p[0].x + p[0].width == p[1].x), "{w}: contiguous");
                        let v = if folded { shown } else { i };
                        let badge = b.pieces.iter().find(|p| p.part == Part::Badge).map(|p| p.text.as_str());
                        assert_eq!(badge, items(icons)[v].badge.as_deref(), "{w} {icons} {folded}: whole");
                    }
                }
            }
        }
    }
    let cut = rows(12, true, 2, false);
    assert_eq!(cut[2], " G_Act… @37 ", "the name is cut, the count kept");
}

#[test]
fn a_glyph_takes_two_cells_and_a_click_anywhere_on_a_row_is_its_view() {
    let b = views(Rect::new(10, 1, 28, 5), &items(true), 2, false);
    let g = b[2].pieces.iter().find(|p| p.part == Part::Glyph).unwrap();
    assert_eq!((g.x, g.width), (11, GLYPH_SLOT));
    let label = b[2].pieces.iter().find(|p| p.part == Part::Label).unwrap();
    assert_eq!(label.x, g.x + GLYPH_SLOT, "the name starts after the slot");
    assert!((10..38).all(|x| b[2].hit(x) == Some(2)), "edge to edge");
    assert_eq!(views(Rect::new(0, 0, 28, 3), &items(true), 0, false).len(), 3, "rows that do not fit are left out");
}

#[test]
fn the_chip_cuts_the_name_then_the_numbers_then_the_name_to_its_letter() {
    let at = |w: u16| text(&chip(Rect::new(1, 0, w, 1), &ws())).trim_end().to_string();
    assert_eq!(at(30), " ▌A company ▾ · B @2");
    assert_eq!(at(20), " ▌A compa… ▾ · B @2", "the name first, eight cells kept");
    assert_eq!(at(15), " ▌A c… ▾ · B @", "then the numbers");
    assert_eq!(at(13), " ▌A ▾ · B @", "then the name to its letter");
    let b = chip(Rect::new(1, 0, 30, 1), &ws());
    assert!(b.pieces.iter().all(|p| p.item.is_none()), "the chip is one thing");
    assert_eq!(b.extent(), (1, 22));
    for w in 0..40 {
        let b = chip(Rect::new(1, 0, w, 1), &ws());
        assert!(b.pieces.iter().map(|p| p.width).sum::<u16>() <= w, "{w}");
    }
}

#[test]
fn a_name_cut_to_one_cell_is_its_letter() {
    assert_eq!(cut_name("A company", 1), "A");
    assert_eq!(cut_name("A company", 6), "A com…");
    assert_eq!(cut_name("A company", 4), "A c…");
    assert_eq!(cut_name("Ab cd", 4), "Ab…", "never a space before the cut");
    assert_eq!(cut_name("A company", 3), "A");
    assert_eq!(compact("@12"), "@");
}
