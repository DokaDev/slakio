use super::*;

const SIZE: Rect = Rect { x: 0, y: 0, width: 120, height: 40 };

fn plain() -> Shape {
    Shape::default()
}

#[test]
fn the_regions_tile_the_screen_above_the_status_line() {
    let a = areas(SIZE, plain());
    assert_eq!(a.status, Rect::new(0, 39, 120, 1));
    assert_eq!(a.rail, Rect::new(0, 0, RAIL_WIDTH, 39));
    let list = a.list.unwrap();
    assert_eq!((list.x, list.width), (RAIL_WIDTH, 26));
    assert_eq!((a.work.x, a.work.right()), (list.right(), 120));
    assert_eq!(list_width(200), 36);
}

#[test]
fn an_expanded_rail_covers_the_list_or_pushes_it_aside() {
    let overlay = areas(SIZE, Shape { rail_expanded: true, ..plain() });
    assert_eq!(overlay.rail.width, RAIL_EXPANDED_WIDTH);
    assert_eq!(overlay.list, areas(SIZE, plain()).list, "the list stays where it was");
    let push = areas(SIZE, Shape { rail_expanded: true, push: true, ..plain() });
    assert_eq!(push.list.unwrap().x, RAIL_EXPANDED_WIDTH);
    let hidden = areas(SIZE, Shape { list_hidden: true, ..plain() });
    assert_eq!((hidden.list, hidden.work.x), (None, RAIL_WIDTH));
}

#[test]
fn a_narrow_screen_gives_the_list_room_to_the_thread_panel() {
    let small = Rect::new(0, 0, 80, 24);
    let a = areas(small, Shape { thread: true, ..plain() });
    assert_eq!(a.list, None, "the list is left out while the thread panel is open");
    let (main, thread) = work_split(a.work, true, false);
    assert!(main.unwrap().width >= MAIN_MIN && thread.unwrap().width >= 34);
    let focused = areas(small, Shape { thread: true, list_focused: true, ..plain() });
    assert!(focused.list.is_some(), "a focused list is never left out");
    // Wide enough: the list stays.
    assert!(areas(SIZE, Shape { thread: true, ..plain() }).list.is_some());
    // Too narrow for both panes: the one with the keyboard takes the work area.
    let narrow = Rect::new(0, 0, 60, 20);
    assert_eq!(work_split(narrow, true, true), (None, Some(narrow)));
    assert_eq!(work_split(narrow, true, false), (Some(narrow), None));
}

#[test]
fn a_pane_has_its_messages_a_divider_and_the_composer() {
    let pane = Rect::new(10, 0, 40, 20);
    let p = pane_parts(pane, 1);
    assert_eq!(p.input, Rect::new(11, 18, 38, 1));
    assert_eq!(p.divider, Some(17));
    assert_eq!(p.messages, Rect::new(11, 1, 38, 16));
    assert_eq!(pane_parts(pane, 9).input.height, COMPOSER_MAX_LINES);
    let low = pane_parts(Rect::new(0, 0, 40, 4), 1);
    assert_eq!(low.divider, None);
    assert_eq!(composer_width(pane), 34);
}

#[test]
fn rail_rows_map_to_items_around_the_separator() {
    let rail = areas(SIZE, plain()).rail;
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
        let a = areas(Rect::new(0, 0, w, h), Shape { rail_expanded: true, thread: true, ..plain() });
        let _ = work_split(a.work, true, true);
        let _ = pane_parts(a.work, 3);
    }
}
