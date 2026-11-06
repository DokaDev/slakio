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

#[test]
fn a_tree_mapped_to_new_panes_keeps_its_shape() {
    let got = beside().map(&|id| PaneId(id.0 + 10));
    assert_eq!(got, Node::split(Dir::Row, THREAD, Node::Leaf(PaneId(11)), Node::Leaf(PaneId(12))));
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

const HALF: Share = Share { num: 1, den: 2, min: 1, max: 400, keep: 1 };

/// `1 | (2 / 3)`: 1 on the left, 2 above 3 on the right.
fn three() -> Node {
    let right = Node::split(Dir::Column, HALF, Node::Leaf(PaneId(2)), Node::Leaf(PaneId(3)));
    Node::split(Dir::Row, HALF, Node::Leaf(PaneId(1)), right)
}

#[test]
fn the_part_around_panes_is_the_smallest_that_holds_them_all() {
    let t = three();
    assert_eq!(t.around(&[PaneId(2)]), Some(&Node::Leaf(PaneId(2))));
    assert_eq!(t.around(&[PaneId(2), PaneId(3)]).map(Node::leaves), Some(vec![PaneId(2), PaneId(3)]));
    assert_eq!(t.around(&[PaneId(1), PaneId(3)]), Some(&t));
    assert_eq!(t.around(&[PaneId(9)]), None);
    assert_eq!(t.around(&[]), None);
    let part = t.around(&[PaneId(2), PaneId(3)]).unwrap().clone();
    let got = t.replace_part(&part, &Node::split(Dir::Row, HALF, part.clone(), Node::Leaf(PaneId(4))));
    assert_eq!(got.leaves(), vec![PaneId(1), PaneId(2), PaneId(3), PaneId(4)]);
    assert_eq!(got.without(PaneId(4)), Some(t));
}

#[test]
fn equalized_a_run_of_splits_the_same_way_gives_each_part_the_same_room() {
    let share = |_: Dir, a: u16, b: u16| Share { num: b, den: a + b, min: 1, max: 400, keep: 1 };
    let none = |_: &Share| false;
    // 1 | (2 | 3): three columns of a third each.
    let lopsided = Share { num: 9, den: 10, min: 1, max: 400, keep: 1 };
    let row = Node::split(
        Dir::Row,
        lopsided,
        Node::Leaf(PaneId(1)),
        Node::split(Dir::Row, lopsided, Node::Leaf(PaneId(2)), Node::Leaf(PaneId(3))),
    );
    let got = row.equalized(&none, &share).solve(area(90, 10), None);
    assert_eq!(got.iter().map(|(_, a)| a.width).collect::<Vec<_>>(), vec![30, 30, 30]);
    // A split the other way counts as one part; a share kept stays and counts as one.
    let got = three().equalized(&none, &share).solve(area(90, 10), None);
    assert_eq!(got.iter().map(|(_, a)| (a.width, a.height)).collect::<Vec<_>>(), vec![(45, 10), (45, 5), (45, 5)]);
    let kept = Node::split(
        Dir::Row,
        HALF,
        Node::Leaf(PaneId(1)),
        Node::split(Dir::Row, THREAD, Node::Leaf(PaneId(2)), Node::Leaf(PaneId(3))),
    );
    let got = kept.equalized(&|s: &Share| *s == THREAD, &share);
    let Node::Split { share: outer, second, .. } = &got else { panic!("a split") };
    assert_eq!((outer.num, outer.den), (1, 2));
    assert!(matches!(**second, Node::Split { share, .. } if share == THREAD));
}

#[test]
fn toward_finds_the_pane_beside_by_place() {
    let laid = three().solve(area(80, 20), None);
    let go = |from: u64, t: Toward| toward(&laid, PaneId(from), t).map(|p| p.0);
    assert_eq!(go(1, Toward::Right), Some(2), "of 2 and 3, the first (top)");
    assert_eq!(go(3, Toward::Left), Some(1));
    assert_eq!(go(2, Toward::Down), Some(3));
    assert_eq!(go(3, Toward::Up), Some(2));
    assert_eq!(go(1, Toward::Left), None);
    assert_eq!(go(1, Toward::Up), None);
    assert_eq!(go(2, Toward::Right), None);
    assert_eq!(go(9, Toward::Right), None, "a pane not laid out");
}

/// A tree of panes `1..=n` made by splitting one of them at a time, either way.
fn grown() -> impl Strategy<Value = Node> {
    prop::collection::vec((0usize..8, any::<bool>()), 0..8).prop_map(|steps| {
        let mut t = Node::Leaf(PaneId(1));
        for (k, (at, row)) in steps.into_iter().enumerate() {
            let leaves = t.leaves();
            let id = leaves[at % leaves.len()];
            let dir = if row { Dir::Row } else { Dir::Column };
            t = t.replace(id, &Node::split(dir, HALF, Node::Leaf(id), Node::Leaf(PaneId(k as u64 + 2))));
        }
        t
    })
}

proptest! {
    /// Going a way and back comes back to a pane on the side started from, and every pane
    /// laid out is reached from any other by going toward it.
    #[test]
    fn toward_moves_between_panes_that_touch(t in grown(), w in 20u16..300, h in 10u16..120) {
        let laid = t.solve(area(w, h), None);
        for (id, _) in &laid {
            for (there, back) in [(Toward::Left, Toward::Right), (Toward::Right, Toward::Left), (Toward::Up, Toward::Down), (Toward::Down, Toward::Up)] {
                if let Some(to) = toward(&laid, *id, there) {
                    prop_assert_ne!(to, *id);
                    prop_assert!(toward(&laid, to, back).is_some(), "{:?} back from {:?}", back, to);
                }
            }
        }
    }

    /// Equalized, a tree keeps its panes and their order, and still tiles the area.
    #[test]
    fn equalized_keeps_the_panes(t in grown(), w in 0u16..300, h in 0u16..120) {
        let share = |_: Dir, a: u16, b: u16| Share { num: b, den: a + b, min: 1, max: u16::MAX, keep: 1 };
        let e = t.equalized(&|_: &Share| false, &share);
        prop_assert_eq!(e.leaves(), t.leaves());
        let cells: u64 = e.solve(area(w, h), None).iter().map(|(_, r)| u64::from(r.width) * u64::from(r.height)).sum();
        prop_assert_eq!(cells, u64::from(w) * u64::from(h));
    }
}
