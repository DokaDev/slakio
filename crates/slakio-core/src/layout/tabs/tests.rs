use super::*;
use crate::layout::{Dir, Share};
use proptest::prelude::*;

const HALF: Share = Share { num: 1, den: 2, min: 1, max: 400, keep: 1 };

/// A tab of pane `id`, named `t<id>` so tests can follow it.
fn tab(id: u64) -> Tab {
    Tab { name: Some(format!("t{id}")), ..Tab::new(PaneId(id)) }
}

fn names(t: &Tabs) -> Vec<String> {
    t.all().iter().map(|t| t.name.clone().unwrap_or_default()).collect()
}

#[test]
fn a_new_tab_goes_right_after_the_one_shown_and_is_shown() {
    let mut t = Tabs::default();
    assert_eq!((t.current(), t.current_tab()), (None, None));
    assert_eq!(t.open(tab(1)), 0);
    t.open(tab(2));
    t.select(0);
    assert_eq!(t.open(tab(3)), 1);
    assert_eq!(names(&t), ["t1", "t3", "t2"]);
    assert_eq!(t.current(), Some(1));
}

#[test]
fn closing_shows_the_tab_to_the_right_else_the_new_last_and_the_last_leaves_none() {
    let mut t = Tabs::default();
    for id in 1..=3 {
        t.open(tab(id));
    }
    t.select(1);
    assert_eq!(t.close(1).unwrap().name.as_deref(), Some("t2"));
    assert_eq!(t.current_tab().unwrap().name.as_deref(), Some("t3"), "the one to the right");
    t.close(1);
    assert_eq!(t.current_tab().unwrap().name.as_deref(), Some("t1"), "else the new last");
    t.open(tab(4));
    t.select(1);
    t.close(0);
    assert_eq!(t.current_tab().unwrap().name.as_deref(), Some("t4"), "a tab left of it closing keeps it shown");
    t.close(0);
    assert!(t.is_empty() && t.current().is_none() && t.close(0).is_none());
}

#[test]
fn closing_a_pane_keeps_its_tab_until_the_last_one_goes() {
    let mut t = Tabs::default();
    t.open(Tab { root: Node::split(Dir::Row, HALF, Node::Leaf(PaneId(1)), Node::Leaf(PaneId(2))), ..tab(1) });
    t.open(tab(3));
    t.current_tab_mut().unwrap().active = PaneId(3);
    assert_eq!(t.close_pane(PaneId(1), None), Closed::Pane(0));
    assert_eq!(t.all()[0].active, PaneId(2), "the active pane moves to one still there");
    let Closed::Tab(0, closed) = t.close_pane(PaneId(2), None) else { panic!() };
    assert_eq!(closed.name.as_deref(), Some("t1"));
    assert_eq!(names(&t), ["t3"]);
    assert_eq!(t.close_pane(PaneId(9), None), Closed::Nothing);
}

#[test]
fn moves_are_the_users_and_the_tab_shown_stays_shown() {
    let mut t = Tabs::default();
    for id in 1..=4 {
        t.open(tab(id));
    }
    t.select(1);
    assert!(t.shift(1));
    assert_eq!(names(&t), ["t1", "t3", "t2", "t4"]);
    assert_eq!(t.current(), Some(2));
    assert!(t.move_tab(3, 0));
    assert_eq!(names(&t), ["t4", "t1", "t3", "t2"]);
    assert_eq!(t.current_tab().unwrap().name.as_deref(), Some("t2"));
    t.select(0);
    assert!(!t.shift(-1), "the first tab goes no further left");
    assert!(!t.move_tab(0, 0) && !t.move_tab(0, 9));
    t.step(-1);
    assert_eq!(t.current(), Some(3), "stepping goes round");
}

/// What a test does to the tabs.
#[derive(Clone, Debug)]
enum Op {
    Open,
    Close(usize),
    ClosePane(u64),
    Split(usize),
    Select(usize),
    Step(isize),
    Shift(isize),
    Move(usize, usize),
    Reopen,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        Just(Op::Open),
        (0usize..6).prop_map(Op::Close),
        (1u64..30).prop_map(Op::ClosePane),
        (0usize..6).prop_map(Op::Split),
        (0usize..6).prop_map(Op::Select),
        (-3isize..4).prop_map(Op::Step),
        (-2isize..3).prop_map(Op::Shift),
        (0usize..6, 0usize..6).prop_map(|(a, b)| Op::Move(a, b)),
        Just(Op::Reopen),
    ]
}

proptest! {
    /// Whatever happens, no tab is empty, a pane is in one tab, a tab is shown while there is
    /// one, and only the user's moves change the order of the tabs that stay.
    #[test]
    fn tabs_hold_and_only_moves_reorder(ops in prop::collection::vec(op(), 0..60)) {
        let mut t = Tabs::default();
        let mut next = 1u64;
        let mut closed: Vec<(usize, Tab)> = Vec::new();
        for op in ops {
            let before = names(&t);
            let mut moved = false;
            match op {
                Op::Open => {
                    t.open(tab(next));
                    next += 1;
                }
                Op::Close(i) => {
                    if let Some(c) = t.close(i) {
                        closed.push((i, c));
                    }
                }
                Op::ClosePane(p) => {
                    if let Closed::Tab(i, c) = t.close_pane(PaneId(p), None) {
                        prop_assert!(!c.root.contains(PaneId(p)) || c.root.leaves() == vec![PaneId(p)]);
                        closed.push((i, c));
                    }
                }
                Op::Split(i) => {
                    if let Some(tb) = t.get_mut(i) {
                        tb.root = Node::split(Dir::Row, HALF, tb.root.clone(), Node::Leaf(PaneId(next)));
                        next += 1;
                    }
                }
                Op::Select(i) => {
                    t.select(i);
                }
                Op::Step(by) => t.step(by),
                Op::Shift(by) => moved = t.shift(by),
                Op::Move(a, b) => moved = t.move_tab(a, b),
                Op::Reopen => {
                    if let Some((i, c)) = closed.pop() {
                        let name = c.name.clone();
                        let at = t.insert(i, c);
                        prop_assert_eq!(&t.current_tab().unwrap().name, &name, "a reopened tab keeps its name, shown");
                        prop_assert!(at <= i);
                    }
                }
            }
            prop_assert!(t.holds(), "{:?}", t);
            if !moved {
                // The tabs there before and after keep their order.
                let after = names(&t);
                let kept_before: Vec<&String> = before.iter().filter(|n| after.contains(n)).collect();
                let kept_after: Vec<&String> = after.iter().filter(|n| before.contains(n)).collect();
                prop_assert_eq!(kept_before, kept_after);
            }
        }
        // Closing every pane, one by one, ends with no tab and never fails.
        for p in t.panes() {
            t.close_pane(p, None);
            prop_assert!(t.holds());
        }
        prop_assert!(t.is_empty());
    }
}
