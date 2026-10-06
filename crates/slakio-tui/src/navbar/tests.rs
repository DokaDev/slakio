use super::*;

const ICONS: [&str; 5] = ["\u{F02DC}", "\u{F0361}", "\u{F009A}", "\u{F0219}", "\u{F00C0}"];

fn views(icons: bool) -> Vec<Item> {
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

fn chip() -> Chip {
    Chip { name: "A company".to_string(), others: vec![("B".to_string(), "@2".to_string())] }
}

/// The bar as text; a glyph slot shows as `G_` (its two cells).
fn text(b: &Bar) -> String {
    let mut s = String::new();
    for p in &b.pieces {
        s.push_str(if p.part == Part::Glyph { "G_" } else { &p.text });
    }
    s.trim_end().to_string()
}

#[test]
fn with_room_the_bar_says_everything() {
    let area = Rect::new(0, 0, 120, 1);
    assert_eq!(
        text(&layout(area, &chip(), &views(false))),
        " ▌A company ▾ · B @2 │ Home  DMs ●2  Activity @3  Files  Later"
    );
    assert_eq!(
        text(&layout(area, &chip(), &views(true))),
        " ▌A company ▾ · B @2 │ G_ Home  G_ DMs ●2  G_ Activity @3  G_ Files  G_ Later"
    );
}

#[test]
fn narrower_it_cuts_the_name_drops_glyphs_then_names_then_numbers_and_never_wraps() {
    let at = |w: u16, icons: bool| text(&layout(Rect::new(0, 0, w, 1), &chip(), &views(icons)));
    assert_eq!(at(64, false), " ▌A company ▾ · B @2 │ Home  DMs ●2  Activity @3  Files  Later");
    assert_eq!(at(62, false), " ▌A compa… ▾ · B @2 │ Home  DMs ●2  Activity @3  Files  Later", "the name first");
    assert_eq!(at(60, false), " ▌A company ▾ · B @2 │ H  D ●2  A @3  F  L", "then letters (icons off)");
    assert_eq!(at(74, true), " ▌A company ▾ · B @2 │ Home  DMs ●2  Activity @3  Files  Later", "words before glyphs");
    assert_eq!(at(60, true), " ▌A company ▾ · B @2 │ G_ G_ ●2  G_ @3  G_ G_", "then glyphs alone, two cells apart");
    assert_eq!(at(40, false), " ▌A company ▾ · B @ │ H  D ●  A @  F  L", "then counts without numbers");
    assert_eq!(at(34, false), " ▌A ▾ · B @ │ H  D ●  A @  F  L", "then the name's letter");
    assert_eq!(at(80, true), " ▌A company ▾ · B @2 │ G_ Home  G_ DMs ●2  G_ Activity @3  G_ Files  G_ Later");
    for w in 0..200 {
        for icons in [false, true] {
            let b = layout(Rect::new(3, 0, w, 1), &chip(), &views(icons));
            let used: u16 = b.pieces.iter().map(|p| p.width).sum();
            assert!(used <= w, "{w} {icons}: {used}");
            assert!(b.pieces.windows(2).all(|p| p[0].x + p[0].width == p[1].x), "{w}: contiguous");
        }
    }
}

#[test]
fn a_glyph_takes_two_cells_and_a_click_on_either_is_its_view() {
    let b = layout(Rect::new(10, 0, 120, 1), &chip(), &views(true));
    let g = b.pieces.iter().find(|p| p.part == Part::Glyph && p.item == Some(2)).unwrap();
    assert_eq!(g.width, GLYPH_SLOT);
    assert_eq!((b.hit(g.x), b.hit(g.x + 1)), (Some(2), Some(2)), "the glyph's cell and the blank after it");
    let label = b.pieces.iter().find(|p| p.part == Part::Label && p.item == Some(2)).unwrap();
    assert_eq!(label.x, g.x + GLYPH_SLOT + 1, "the name starts after the slot and a space");
    assert_eq!(b.hit(10), Some(0), "the chip");
    let sep = b.pieces.iter().find(|p| p.part == Part::Sep).unwrap();
    assert_eq!(b.hit(sep.x), None);
    let (from, to) = b.span(2).unwrap();
    assert!(from <= g.x && to > label.x);
    // A glyph that does not fit whole is left out, never cut in half.
    for w in 0..120 {
        let b = layout(Rect::new(0, 0, w, 1), &chip(), &views(true));
        assert!(b.pieces.iter().filter(|p| p.part == Part::Glyph).all(|p| p.width == GLYPH_SLOT && p.x + 2 <= w));
        // After a glyph's slot comes a blank (or nothing): text never starts right after a
        // glyph, so a glyph drawn two cells wide that the terminal redraws alone covers no text.
        for pair in b.pieces.windows(2).filter(|p| p[0].part == Part::Glyph) {
            assert!(
                matches!(pair[1].part, Part::Blank | Part::Badge) && pair[1].text.starts_with(' '),
                "{w}: {pair:?}"
            );
        }
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
