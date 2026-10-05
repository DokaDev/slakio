use super::*;

const SIZE: Rect = Rect { x: 0, y: 0, width: 120, height: 40 };

#[test]
fn the_regions_tile_the_screen_above_the_status_line() {
    let a = areas(SIZE, false, false, false);
    assert_eq!(a.status, Rect::new(0, 39, 120, 1));
    assert_eq!(a.rail, Rect::new(0, 0, RAIL_WIDTH, 39));
    let list = a.list.unwrap();
    assert_eq!((list.x, list.width), (RAIL_WIDTH, 24));
    assert_eq!((a.work.x, a.work.right()), (list.right(), 120));
}

#[test]
fn an_expanded_rail_covers_the_list_or_pushes_it_aside() {
    let overlay = areas(SIZE, true, false, false);
    assert_eq!(overlay.rail.width, RAIL_EXPANDED_WIDTH);
    assert_eq!(overlay.list, areas(SIZE, false, false, false).list, "the list stays where it was");
    let push = areas(SIZE, true, true, false);
    assert_eq!(push.list.unwrap().x, RAIL_EXPANDED_WIDTH);
    let hidden = areas(SIZE, false, false, true);
    assert_eq!((hidden.list, hidden.work.x), (None, RAIL_WIDTH));
}

#[test]
fn rail_rows_map_to_items_around_the_separator() {
    let rail = areas(SIZE, false, false, false).rail;
    // Row 0 is the border; two workspaces, the separator, then five views.
    assert_eq!(rail_item_at(rail, 2, 7, 0), None);
    assert_eq!(rail_item_at(rail, 2, 7, 1), Some(0));
    assert_eq!(rail_item_at(rail, 2, 7, 2), Some(1));
    assert_eq!(rail_item_at(rail, 2, 7, 3), None);
    assert_eq!(rail_item_at(rail, 2, 7, 4), Some(2));
    assert_eq!(rail_item_at(rail, 2, 7, 8), Some(6));
    assert_eq!(rail_item_at(rail, 2, 7, 9), None);
}

#[test]
fn small_screens_are_too_small_and_never_panic() {
    assert!(too_small(Rect::new(0, 0, MIN_WIDTH - 1, 40)));
    assert!(too_small(Rect::new(0, 0, 120, MIN_HEIGHT - 1)));
    assert!(!too_small(Rect::new(0, 0, MIN_WIDTH, MIN_HEIGHT)));
    for (w, h) in [(0, 0), (1, 1), (3, 2)] {
        let _ = areas(Rect::new(0, 0, w, h), true, false, false);
    }
}
