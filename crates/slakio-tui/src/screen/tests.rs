use super::*;

const SIZE: Rect = Rect { x: 0, y: 0, width: 120, height: 40 };

fn plain() -> Shape {
    Shape::default()
}

#[test]
fn the_regions_tile_the_screen_under_the_top_bar_and_above_the_status_line() {
    let a = areas(SIZE, plain());
    assert_eq!(a.nav, Rect::new(0, 0, 120, 1), "the top bar, the whole width");
    assert_eq!(a.status, Rect::new(0, 39, 120, 1));
    let list = a.list.unwrap();
    assert_eq!(list, Rect::new(0, 1, 26, 38), "the list from the left edge");
    assert_eq!((a.work.x, a.work.y, a.work.right()), (list.right(), 1, 120));
    assert_eq!(list_width(200), 36);
    let hidden = areas(SIZE, Shape { list_hidden: true, ..plain() });
    assert_eq!((hidden.list, hidden.work.x, hidden.work.width), (None, 0, 120));
    let tabs = areas(SIZE, Shape { tabs: true, ..plain() });
    assert_eq!((tabs.tabs, tabs.work.y), (Some(Rect::new(26, 1, 94, 1)), 2), "the tab bar under the top bar");
}

#[test]
fn a_narrow_screen_gives_the_list_room_to_the_thread_panel() {
    let small = Rect::new(0, 0, 80, 24);
    let a = areas(small, Shape { thread: true, ..plain() });
    assert_eq!(a.list, None, "the list is left out while the thread panel is open");
    let panes = work_panes(a.work, &beside(), None);
    assert!(panes[0].1.width >= MAIN_MIN && panes[1].1.width >= 34);
    let focused = areas(small, Shape { thread: true, list_focused: true, ..plain() });
    assert!(focused.list.is_some(), "a focused list is never left out");
    // Wide enough: the list stays.
    assert!(areas(SIZE, Shape { thread: true, ..plain() }).list.is_some());
    // Too narrow for both panes: the one with the keyboard takes the work area.
    let narrow = Rect::new(0, 0, 60, 20);
    assert_eq!(work_panes(narrow, &beside(), Some(THREAD)), vec![(THREAD, narrow)]);
    assert_eq!(work_panes(narrow, &beside(), Some(MAIN)), vec![(MAIN, narrow)]);
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
fn the_switcher_sits_under_the_top_bar_inside_the_screen() {
    assert_eq!(switcher(SIZE, 1, 2), Rect::new(1, 1, SWITCHER_WIDTH, 4));
    assert_eq!(switcher(Rect::new(0, 0, 50, 10), 40, 2).right(), 50, "kept on screen");
    assert_eq!(switcher(Rect::new(0, 0, 20, 10), 0, 30).height, 8);
}

#[test]
fn small_screens_are_too_small_and_never_panic() {
    assert!(too_small(Rect::new(0, 0, MIN_WIDTH - 1, 40)));
    assert!(too_small(Rect::new(0, 0, 120, MIN_HEIGHT - 1)));
    assert!(!too_small(Rect::new(0, 0, MIN_WIDTH, MIN_HEIGHT)));
    for (w, h) in [(0, 0), (1, 1), (3, 2)] {
        let a = areas(Rect::new(0, 0, w, h), Shape { tabs: true, thread: true, ..plain() });
        let _ = work_panes(a.work, &beside(), Some(THREAD));
        let _ = pane_parts(a.work, 3);
    }
}

const MAIN: PaneId = PaneId(1);
const THREAD: PaneId = PaneId(2);

/// The conversation with the thread panel beside it.
fn beside() -> Node {
    Node::split(slakio_core::layout::Dir::Row, THREAD_SHARE, Node::Leaf(MAIN), Node::Leaf(THREAD))
}

/// One line in the conversation's composer, `thread_lines` in the thread panel's.
fn lines(thread_lines: usize) -> impl Fn(PaneId, usize) -> usize {
    move |id, _| if id == THREAD { thread_lines } else { 1 }
}

/// The split of the work area as it was worked out before the layout tree, kept to check the
/// tree gives every width the same rectangles.
fn split_before(work: Rect, thread_focused: bool) -> Vec<(PaneId, Rect)> {
    let w = (work.width * 2 / 5).clamp(34, 60);
    if work.width < w + MAIN_MIN {
        return vec![if thread_focused { (THREAD, work) } else { (MAIN, work) }];
    }
    let [main, side] = Layout::horizontal([Constraint::Min(0), Constraint::Length(w)]).areas(work);
    vec![(MAIN, main), (THREAD, side)]
}

#[test]
fn the_layout_tree_splits_the_work_area_as_before_at_every_width() {
    for width in 0..=400 {
        for focused in [MAIN, THREAD] {
            let work = Rect::new(30, 0, width, 40);
            let want = split_before(work, focused == THREAD);
            assert_eq!(work_panes(work, &beside(), Some(focused)), want, "{width} wide");
        }
    }
}

#[test]
fn the_frame_lays_out_each_open_pane_with_its_composer() {
    for (w, h) in [(80, 24), (120, 40), (200, 50)] {
        let size = Rect::new(0, 0, w, h);
        let shape = Shape { thread: true, ..plain() };
        let f = frame(size, shape, Some(&beside()), Some(MAIN), lines(3));
        assert_eq!(f.areas, areas(size, shape), "{w}x{h}");
        let ids: Vec<PaneId> = f.panes.iter().map(|p| p.id).collect();
        assert_eq!(ids, [MAIN, THREAD], "{w}x{h}");
        let m = f.pane(MAIN).unwrap();
        let t = f.pane(THREAD).unwrap();
        assert_eq!(vec![(MAIN, m.rect), (THREAD, t.rect)], work_panes(f.areas.work, &beside(), None), "{w}x{h}");
        assert_eq!(m.parts, pane_parts(m.rect, 1), "{w}x{h}");
        assert_eq!(t.parts, pane_parts(t.rect, 3), "{w}x{h}");
        // The panes tile the work area side by side.
        assert_eq!((m.rect.x, m.rect.right(), t.rect.right()), (f.areas.work.x, t.rect.x, f.areas.work.right()));
        let inside = Position { x: t.rect.x + 1, y: t.rect.y + 1 };
        assert_eq!(f.pane_at(inside).map(|p| p.id), Some(THREAD));
        assert_eq!(f.pane_at(Position { x: 0, y: 0 }), None, "the top bar is no pane");
    }
}

#[test]
fn the_frame_shows_no_pane_without_a_conversation_and_one_where_two_do_not_fit() {
    let f = frame(SIZE, plain(), None, None, lines(1));
    assert!(f.panes.is_empty());
    let narrow = Rect::new(0, 0, 60, 20);
    let shape = Shape { thread: true, list_hidden: true, ..plain() };
    for focused in [MAIN, THREAD] {
        let f = frame(narrow, shape, Some(&beside()), Some(focused), lines(2));
        assert_eq!(f.panes.len(), 1);
        assert_eq!((f.panes[0].id, f.panes[0].rect), (focused, f.areas.work));
    }
    let main_only = frame(SIZE, plain(), Some(&Node::Leaf(MAIN)), Some(MAIN), lines(1));
    assert_eq!(main_only.panes.len(), 1);
    assert_eq!(main_only.panes[0].rect, main_only.areas.work);
    // A screen too small to draw still lays out without panicking.
    let _ = frame(Rect::new(0, 0, 3, 2), Shape { thread: true, ..plain() }, Some(&beside()), Some(THREAD), lines(5));
}
