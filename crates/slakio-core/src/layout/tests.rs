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

/// The least of a pane: 4 columns, 2 rows.
fn least(d: Dir) -> u16 {
    if d == Dir::Row { 4 } else { 2 }
}

fn never(_: &Share) -> bool {
    false
}

#[test]
fn fitted_shares_need_what_their_sides_hold() {
    // 1 | (2 | 3): the outer split's second side needs two panes' room.
    let t = Node::split(
        Dir::Row,
        HALF,
        Node::Leaf(PaneId(1)),
        Node::split(Dir::Row, HALF, Node::Leaf(PaneId(2)), Node::Leaf(PaneId(3))),
    );
    let f = t.fitted(&never, &least);
    let Node::Split { share, second, .. } = &f else { panic!("a split") };
    assert_eq!((share.min, share.keep, share.num, share.den), (8, 4, HALF.num, HALF.den));
    assert!(matches!(**second, Node::Split { share, .. } if (share.min, share.keep) == (4, 4)));
    // Shrunk, every pane keeps its least before one gives way.
    let got = f.solve(area(12, 5), None);
    assert_eq!(got.iter().map(|(_, a)| a.width).collect::<Vec<_>>(), vec![4, 4, 4]);
    assert_eq!(f.solve(area(11, 5), Some(PaneId(3))).len(), 2, "the side with the keyboard stays");
    // A split the other way needs its larger side; a kept split one pane.
    assert_eq!(three().needs(Dir::Row, &never, &least), 8);
    assert_eq!(three().needs(Dir::Column, &never, &least), 4);
    assert_eq!(beside().needs(Dir::Row, &|s: &Share| *s == THREAD, &least), 4);
    assert_eq!(beside().fitted(&|s: &Share| *s == THREAD, &least), beside());
}

#[test]
fn the_sibling_is_what_takes_the_room() {
    let t = three();
    let two = Node::Leaf(PaneId(2));
    assert_eq!(t.sibling(&two), Some(&Node::Leaf(PaneId(3))));
    assert_eq!(t.sibling(&Node::Leaf(PaneId(1))).map(Node::leaves), Some(vec![PaneId(2), PaneId(3)]));
    assert_eq!(t.sibling(&t), None);
    assert_eq!(t.sibling(&Node::Leaf(PaneId(9))), None);
}

#[test]
fn seams_are_the_last_line_of_each_first_side() {
    let t = three();
    let seams = t.seams(area(80, 20), None);
    assert_eq!(
        seams,
        vec![
            Seam { dir: Dir::Row, at: 7 + 40 - 1, from: 3, to: 23, id: PaneId(2) },
            Seam { dir: Dir::Column, at: 3 + 10 - 1, from: 47, to: 87, id: PaneId(3) },
        ]
    );
    // A split one side of which gives way has no seam.
    assert_eq!(beside().seams(area(60, 20), None), vec![]);
    let z = Node::Leaf(PaneId(1));
    assert_eq!(z.seams(area(60, 20), None), vec![]);
}

#[test]
fn a_seam_dragged_moves_within_the_least_sizes() {
    let t = three().fitted(&never, &least);
    let a = area(80, 20);
    let moved = t.drag(a, None, PaneId(2), 7 + 59).expect("moves");
    let w: Vec<u16> = moved.solve(a, None).iter().map(|(_, r)| r.width).collect();
    assert_eq!(w, vec![60, 20, 20]);
    assert_eq!(moved.seams(a, None)[0].at, 7 + 59);
    // As far as the right side keeps its least.
    let far = t.drag(a, None, PaneId(2), 200).unwrap();
    assert_eq!(far.solve(a, None)[0].1.width, 76);
    let near = t.drag(a, None, PaneId(2), 0).unwrap();
    assert_eq!(near.solve(a, None)[0].1.width, 4);
    // Rows the same way; a seam not laid out, or not moved, changes nothing.
    let down = t.drag(a, None, PaneId(3), 3 + 4).unwrap();
    assert_eq!(down.solve(a, None)[1].1.height, 5);
    assert_eq!(t.drag(a, None, PaneId(9), 10), None);
    assert_eq!(t.drag(a, None, PaneId(2), 7 + 39), None);
}

#[test]
fn a_pane_resized_takes_room_from_the_nearest_split_that_way() {
    let t = three().fitted(&never, &least);
    let a = area(80, 20);
    let width = |n: &Node, id: u64| n.solve(a, None).into_iter().find(|(p, _)| p.0 == id).unwrap().1;
    // 2 is the second side: wider moves the seam left; 1 is the first: wider moves it right.
    let got = t.resize(a, None, PaneId(2), Dir::Row, 4).unwrap();
    assert_eq!((width(&got, 1).width, width(&got, 2).width), (36, 44));
    let got = t.resize(a, None, PaneId(1), Dir::Row, 4).unwrap();
    assert_eq!((width(&got, 1).width, width(&got, 2).width), (44, 36));
    let got = t.resize(a, None, PaneId(2), Dir::Column, -2).unwrap();
    assert_eq!((width(&got, 2).height, width(&got, 3).height), (8, 12));
    // No split that way around it, or none that can give: nothing.
    assert_eq!(t.resize(a, None, PaneId(1), Dir::Column, 2), None);
    assert_eq!(t.resize(a, None, PaneId(1), Dir::Row, 500).map(|n| width(&n, 1).width), Some(76));
    let full = t.resize(a, None, PaneId(1), Dir::Row, 500).unwrap();
    assert_eq!(full.resize(a, None, PaneId(1), Dir::Row, 4), None);
}

proptest! {
    /// Resized or dragged, a fitted tree keeps its panes, tiles the area, and every pane it
    /// showed keeps its least room.
    #[test]
    fn resizing_keeps_every_pane_its_least(t in grown(), w in 0u16..300, h in 0u16..120, by in -400i32..400, pick in 0usize..9, row in any::<bool>()) {
        let t = t.fitted(&never, &least);
        let a = area(w, h);
        let shown = t.solve(a, None).len();
        let leaves = t.leaves();
        let pane = leaves[pick % leaves.len()];
        let dir = if row { Dir::Row } else { Dir::Column };
        let mut changed = vec![t.resize(a, None, pane, dir, by)];
        if let Some(s) = t.seams(a, None).get(pick % 3) {
            changed.push(t.drag(a, None, s.id, s.at.wrapping_add(by as u16)));
        }
        for n in changed.into_iter().flatten() {
            prop_assert_eq!(n.leaves(), t.leaves());
            let laid = n.solve(a, None);
            prop_assert_eq!(laid.len(), shown, "as many panes shown");
            let cells: u64 = laid.iter().map(|(_, r)| u64::from(r.width) * u64::from(r.height)).sum();
            prop_assert_eq!(cells, u64::from(w) * u64::from(h));
            if shown > 1 {
                for (_, r) in &laid {
                    prop_assert!(r.width >= least(Dir::Row) || r.width == w, "{:?}", r);
                    prop_assert!(r.height >= least(Dir::Column) || r.height == h, "{:?}", r);
                }
            }
        }
    }

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
    let none = |_: &Share| None;
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
    let got = kept.equalized(&|s: &Share| (*s == THREAD).then_some(*s), &share);
    let Node::Split { share: outer, second, .. } = &got else { panic!("a split") };
    assert_eq!((outer.num, outer.den), (1, 2));
    assert!(matches!(**second, Node::Split { share, .. } if share == THREAD));
}

#[test]
fn toward_finds_the_pane_beside_by_place() {
    let laid = three().solve(area(80, 20), None);
    let go = |from: u64, t: Toward| toward(&laid, PaneId(from), t, |_| 0).map(|p| p.0);
    assert_eq!(go(1, Toward::Right), Some(2), "of 2 and 3, the first (top)");
    assert_eq!(go(3, Toward::Left), Some(1));
    assert_eq!(go(2, Toward::Down), Some(3));
    assert_eq!(go(3, Toward::Up), Some(2));
    assert_eq!(go(1, Toward::Left), None);
    assert_eq!(go(1, Toward::Up), None);
    assert_eq!(go(2, Toward::Right), None);
    assert_eq!(go(9, Toward::Right), None, "a pane not laid out");
    // Of the two that touch it, the one the keyboard was on last.
    let last = |p: PaneId| u64::from(p == PaneId(3));
    assert_eq!(toward(&laid, PaneId(1), Toward::Right, last), Some(PaneId(3)));
    assert_eq!(toward(&laid, PaneId(1), Toward::Right, |_| 0), Some(PaneId(2)));
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
    /// Going a way and back comes back to the pane started from (the one the keyboard was on
    /// last), and, whatever was focused before, to a pane touching the one gone to; every pane
    /// laid out is reached from any other by going toward it.
    #[test]
    fn toward_moves_between_panes_that_touch(t in grown(), w in 20u16..300, h in 10u16..120, seed in any::<u64>()) {
        let laid = t.solve(area(w, h), None);
        // Some order of focus before.
        let before = |p: PaneId| p.0.wrapping_mul(seed | 1) >> 8;
        let ways = [(Toward::Left, Toward::Right), (Toward::Right, Toward::Left), (Toward::Up, Toward::Down), (Toward::Down, Toward::Up)];
        let rect = |id: PaneId| laid.iter().find(|(p, _)| *p == id).map(|(_, a)| *a).unwrap();
        let touch = |a: Area, b: Area| {
            let rows = a.y.max(b.y) < (a.y + a.height).min(b.y + b.height);
            let cols = a.x.max(b.x) < (a.x + a.width).min(b.x + b.width);
            (rows && (a.x + a.width == b.x || b.x + b.width == a.x)) || (cols && (a.y + a.height == b.y || b.y + b.height == a.y))
        };
        for (id, _) in &laid {
            for (there, back) in ways {
                if let Some(to) = toward(&laid, *id, there, before) {
                    prop_assert_ne!(to, *id);
                    prop_assert!(touch(rect(*id), rect(to)), "{:?} from {:?} to {:?} touches", there, id, to);
                    let came = |p: PaneId| u64::from(p == *id);
                    prop_assert_eq!(toward(&laid, to, back, came), Some(*id), "{:?} back from {:?}", back, to);
                    let any = toward(&laid, to, back, before);
                    prop_assert!(any.is_some_and(|q| touch(rect(q), rect(to))), "{:?} back from {:?}: {:?}", back, to, any);
                }
            }
            // Every pane laid out is reached from this one.
            let mut seen = vec![*id];
            let mut next = vec![*id];
            while let Some(p) = next.pop() {
                for (way, _) in ways {
                    if let Some(q) = toward(&laid, p, way, before).filter(|q| !seen.contains(q)) {
                        seen.push(q);
                        next.push(q);
                    }
                }
            }
            prop_assert_eq!(seen.len(), laid.len(), "from {:?}: {:?} of {:?}", id, seen, laid);
        }
    }

    /// Equalized, a tree keeps its panes and their order, and still tiles the area.
    #[test]
    fn equalized_keeps_the_panes(t in grown(), w in 0u16..300, h in 0u16..120) {
        let share = |_: Dir, a: u16, b: u16| Share { num: b, den: a + b, min: 1, max: u16::MAX, keep: 1 };
        let e = t.equalized(&|_: &Share| None, &share);
        prop_assert_eq!(e.leaves(), t.leaves());
        let cells: u64 = e.solve(area(w, h), None).iter().map(|(_, r)| u64::from(r.width) * u64::from(r.height)).sum();
        prop_assert_eq!(cells, u64::from(w) * u64::from(h));
    }
}
