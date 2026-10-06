use super::*;
use proptest::prelude::*;

const THREAD: Share = Share { num: 2, den: 5, min: 34, max: 60, keep: 40 };

fn beside() -> Node {
    Node::split(Dir::Row, THREAD, Node::Leaf(PaneId(1)), Node::Leaf(PaneId(2)))
}

fn area(width: u16, height: u16) -> Area {
    Area { x: 7, y: 3, width, height }
}

#[test]
fn the_thread_panel_takes_two_fifths_beside_the_conversation() {
    let got = beside().solve(area(120, 30), None);
    assert_eq!(got, vec![(PaneId(1), area(72, 30)), (PaneId(2), Area { x: 7 + 72, y: 3, width: 48, height: 30 })]);
    // Clamped to 34..60 cells.
    assert_eq!(beside().solve(area(80, 30), None)[1].1.width, 34);
    assert_eq!(beside().solve(area(300, 30), None)[1].1.width, 60);
}

#[test]
fn where_both_do_not_fit_the_focused_pane_takes_the_area() {
    let narrow = area(60, 20);
    assert_eq!(beside().solve(narrow, Some(PaneId(2))), vec![(PaneId(2), narrow)]);
    assert_eq!(beside().solve(narrow, Some(PaneId(1))), vec![(PaneId(1), narrow)]);
    assert_eq!(beside().solve(narrow, None), vec![(PaneId(1), narrow)]);
}

#[test]
fn a_pane_taken_out_collapses_its_split() {
    let t = beside();
    assert_eq!(t.leaves(), vec![PaneId(1), PaneId(2)]);
    assert_eq!(t.without(PaneId(2)), Some(Node::Leaf(PaneId(1))));
    assert_eq!(t.without(PaneId(1)), Some(Node::Leaf(PaneId(2))));
    assert_eq!(Node::Leaf(PaneId(1)).without(PaneId(1)), None);
    assert!(t.contains(PaneId(2)) && !t.contains(PaneId(3)));
}

#[test]
fn a_leaf_replaced_by_a_split_keeps_the_rest_of_the_tree() {
    let t = beside();
    let inner = Node::split(Dir::Column, THREAD, Node::Leaf(PaneId(2)), Node::Leaf(PaneId(3)));
    let got = t.replace(PaneId(2), &inner);
    assert_eq!(got.leaves(), vec![PaneId(1), PaneId(2), PaneId(3)]);
    assert_eq!(got.without(PaneId(3)), Some(t.clone()));
    assert_eq!(t.replace(PaneId(9), &inner), t, "a pane not in the tree changes nothing");
}

/// A tree of up to three panes, splits of either direction.
fn tree() -> impl Strategy<Value = Node> {
    let share = (1u16..5, 2u16..6, 0u16..40, 0u16..80, 0u16..60).prop_map(|(num, den, min, max, keep)| Share {
        num,
        den,
        min,
        max,
        keep,
    });
    let dir = prop_oneof![Just(Dir::Row), Just(Dir::Column)];
    prop_oneof![
        Just(Node::Leaf(PaneId(1))),
        (dir.clone(), share.clone()).prop_map(|(d, s)| Node::split(d, s, Node::Leaf(PaneId(1)), Node::Leaf(PaneId(2)))),
        (dir.clone(), share.clone(), dir, share).prop_map(|(d1, s1, d2, s2)| {
            let inner = Node::split(d2, s2, Node::Leaf(PaneId(2)), Node::Leaf(PaneId(3)));
            Node::split(d1, s1, Node::Leaf(PaneId(1)), inner)
        }),
    ]
}

proptest! {
    #[test]
    fn the_panes_shown_tile_the_area_without_overlap(
        t in tree(), width in 0u16..400, height in 0u16..200, focus in 0u64..4,
    ) {
        let a = area(width, height);
        let got = t.solve(a, Some(PaneId(focus)));
        prop_assert!(!got.is_empty());
        let cells: u64 = got.iter().map(|(_, r)| u64::from(r.width) * u64::from(r.height)).sum();
        prop_assert_eq!(cells, u64::from(width) * u64::from(height), "they cover the area");
        for (i, (_, r)) in got.iter().enumerate() {
            prop_assert!(r.x >= a.x && r.y >= a.y && r.x + r.width <= a.x + width && r.y + r.height <= a.y + height);
            for (_, o) in &got[i + 1..] {
                let apart = r.x + r.width <= o.x || o.x + o.width <= r.x || r.y + r.height <= o.y || o.y + o.height <= r.y;
                prop_assert!(apart || r.width == 0 || r.height == 0 || o.width == 0 || o.height == 0, "{:?} {:?}", r, o);
            }
        }
        // A focused pane that exists is always shown.
        if t.contains(PaneId(focus)) {
            prop_assert!(got.iter().any(|(id, _)| *id == PaneId(focus)));
        }
    }

    #[test]
    fn side_by_side_each_pane_keeps_its_minimum(width in 0u16..400, focus in 1u64..3) {
        let got = beside().solve(area(width, 10), Some(PaneId(focus)));
        if got.len() == 2 {
            prop_assert!(got[0].1.width >= THREAD.keep && got[1].1.width >= THREAD.min);
            prop_assert!(got[1].1.width <= THREAD.max);
        } else {
            prop_assert_eq!(got, vec![(PaneId(focus), area(width, 10))]);
        }
    }
}
