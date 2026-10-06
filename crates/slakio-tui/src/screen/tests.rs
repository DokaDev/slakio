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

/// One line in the main pane's composer, `thread_lines` in the thread panel's.
fn lines(thread_lines: usize) -> impl Fn(Slot, usize) -> usize {
    move |slot, _| if slot == Slot::Thread { thread_lines } else { 1 }
}

#[test]
fn the_frame_lays_out_each_open_pane_with_its_composer() {
    for (w, h) in [(80, 24), (120, 40), (200, 50)] {
        let size = Rect::new(0, 0, w, h);
        let shape = Shape { main: true, thread: true, ..plain() };
        let f = frame(size, shape, lines(3));
        assert_eq!(f.areas, areas(size, shape), "{w}x{h}");
        let (main, thread) = work_split(f.areas.work, true, false);
        let slots: Vec<Slot> = f.panes.iter().map(|p| p.slot).collect();
        assert_eq!(slots, [Slot::Main, Slot::Thread], "{w}x{h}");
        let m = f.pane(Slot::Main).unwrap();
        let t = f.pane(Slot::Thread).unwrap();
        assert_eq!((Some(m.rect), Some(t.rect)), (main, thread), "{w}x{h}");
        assert_eq!(m.parts, pane_parts(m.rect, 1), "{w}x{h}");
        assert_eq!(t.parts, pane_parts(t.rect, 3), "{w}x{h}");
        // The panes tile the work area side by side.
        assert_eq!((m.rect.x, m.rect.right(), t.rect.right()), (f.areas.work.x, t.rect.x, f.areas.work.right()));
        let inside = Position { x: t.rect.x + 1, y: t.rect.y + 1 };
        assert_eq!(f.pane_at(inside).map(|p| p.slot), Some(Slot::Thread));
        assert_eq!(f.pane_at(Position { x: 0, y: 0 }), None, "the rail is no pane");
    }
}

#[test]
fn the_frame_shows_no_pane_without_a_conversation_and_one_where_two_do_not_fit() {
    let f = frame(SIZE, plain(), lines(1));
    assert!(f.panes.is_empty());
    let narrow = Rect::new(0, 0, 60, 20);
    let shape = Shape { main: true, thread: true, list_hidden: true, ..plain() };
    for (focused, slot) in [(false, Slot::Main), (true, Slot::Thread)] {
        let f = frame(narrow, Shape { thread_focused: focused, ..shape }, lines(2));
        assert_eq!(f.panes.len(), 1);
        assert_eq!((f.panes[0].slot, f.panes[0].rect), (slot, f.areas.work));
    }
    let main_only = frame(SIZE, Shape { main: true, ..plain() }, lines(1));
    assert_eq!(main_only.panes.len(), 1);
    assert_eq!(main_only.panes[0].rect, main_only.areas.work);
    // A screen too small to draw still lays out without panicking.
    let _ = frame(Rect::new(0, 0, 3, 2), Shape { main: true, thread: true, ..plain() }, lines(5));
}
