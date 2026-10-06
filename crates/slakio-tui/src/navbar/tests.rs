use super::*;

const ICONS: [&str; 5] = ["\u{F02DC}", "\u{F0361}", "\u{F009A}", "\u{F0219}", "\u{F00C0}"];

fn items(icons: bool) -> Vec<Item> {
    let names = ["Home", "DMs", "Activity", "Files", "Later"];
    let badges = [None, Some("●2"), Some("@3"), None, None];
    (0..5)
        .map(|i| Item {
            glyph: icons.then_some(ICONS[i]),
            label: names[i].to_string(),
            short: names[i][..1].to_string(),
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
    s.trim_end().to_string()
}

fn row(w: u16, icons: bool, shown: usize) -> String {
    text(&views(Rect::new(0, 1, w, 1), &items(icons), shown))
}

#[test]
fn the_view_switcher_shows_glyphs_and_counts_and_spells_the_view_shown() {
    assert_eq!(row(40, true, 0), " G_Home  G_●2  G_@3  G_ G_", "a bare glyph's slot is one of the two cells apart");
    assert_eq!(row(40, true, 2), " G_ G_●2  G_Activity @3  G_ G_");
    assert_eq!(row(40, false, 0), " Home  D●2  A@3  F  L", "icons off: letters, the view shown spelled");
    assert_eq!(row(40, false, 1), " H  DMs ●2  A@3  F  L");
}

#[test]
fn a_narrow_view_switcher_closes_up_drops_the_numbers_then_is_cut_and_never_wraps() {
    assert_eq!(row(26, false, 2), " H  D●2  Activity @3  F  L");
    assert_eq!(row(25, false, 2), " H D●2 Activity @3 F L", "one cell apart");
    assert_eq!(row(20, false, 2), " H D● Activity @ F L", "then counts without numbers");
    let cut = row(12, false, 2);
    assert!(cut.starts_with(" H D● Activ") && cut.ends_with('…'), "then cut: {cut}");
    for w in 0..60 {
        for icons in [false, true] {
            for shown in 0..5 {
                let b = views(Rect::new(3, 1, w, 1), &items(icons), shown);
                let used: u16 = b.pieces.iter().map(|p| p.width).sum();
                assert!(used <= w, "{w} {icons}: {used}");
                assert!(b.pieces.windows(2).all(|p| p[0].x + p[0].width == p[1].x), "{w}: contiguous");
            }
        }
    }
}

#[test]
fn a_glyph_takes_two_cells_and_a_click_on_either_is_its_view() {
    let b = views(Rect::new(10, 1, 40, 1), &items(true), 2);
    let g = b.pieces.iter().find(|p| p.part == Part::Glyph && p.item == Some(2)).unwrap();
    assert_eq!(g.width, GLYPH_SLOT);
    assert_eq!((b.hit(g.x), b.hit(g.x + 1)), (Some(2), Some(2)), "the glyph's cell and the blank after it");
    let label = b.pieces.iter().find(|p| p.part == Part::Label && p.item == Some(2)).unwrap();
    assert_eq!(label.x, g.x + GLYPH_SLOT, "the name starts after the slot");
    let (from, to) = b.span(2).unwrap();
    assert!(from <= g.x && to > label.x);
    // A glyph that does not fit whole is left out, never cut in half.
    for w in 0..40 {
        let b = views(Rect::new(0, 0, w, 1), &items(true), 0);
        assert!(b.pieces.iter().filter(|p| p.part == Part::Glyph).all(|p| p.width == GLYPH_SLOT && p.x + 2 <= w));
    }
}

#[test]
fn the_chip_cuts_the_name_then_the_numbers_then_the_name_to_its_letter() {
    let at = |w: u16| text(&chip(Rect::new(1, 0, w, 1), &ws()));
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
