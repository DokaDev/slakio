use super::*;
use proptest::prelude::*;

fn conv(ws: &str, c: &str) -> Target {
    Target::Conversation { workspace: WorkspaceId::new(ws), conversation: ConversationId::new(c) }
}

fn thread(ws: &str, c: &str, ts: u64) -> Target {
    Target::Thread { workspace: WorkspaceId::new(ws), conversation: ConversationId::new(c), thread: Ts(ts) }
}

const HALF: Share = Share { num: 1, den: 2, min: 24, max: u16::MAX, keep: 24 };
const PANEL: Share = Share { num: 2, den: 5, min: 34, max: 60, keep: 40 };

/// Two tabs: a pane with its thread panel beside a pane under it; and one pane.
fn two_tabs() -> Saved {
    let root = Node::split(
        Dir::Column,
        HALF,
        Node::split(Dir::Row, PANEL, Node::Leaf(PaneId(0)), Node::Leaf(PaneId(1))),
        Node::Leaf(PaneId(2)),
    );
    let first = SavedTab {
        name: Some("Incident".to_string()),
        root,
        active: PaneId(1),
        zoomed: true,
        panes: vec![
            SavedPane { target: conv("T1", "C1"), panel: Some(PaneId(1)) },
            SavedPane { target: thread("T1", "C1", 1_700_000_000_000_100), panel: None },
            SavedPane { target: conv("T2", "D9"), panel: None },
        ],
    };
    let second = SavedTab {
        name: None,
        root: Node::Leaf(PaneId(0)),
        active: PaneId(0),
        zoomed: false,
        panes: vec![SavedPane { target: conv("T2", "C2"), panel: None }],
    };
    Saved { tabs: vec![first, second], current: 1 }
}

#[test]
fn a_layout_written_reads_back_the_same() {
    let saved = two_tabs();
    let text = saved.to_text();
    assert!(text.contains("version = 1"), "{text}");
    assert!(text.contains("\"row 2 5 34 60 40\""), "{text}");
    assert_eq!(Saved::parse(&text), Ok(saved));
    assert_eq!(Saved::parse(&Saved::default().to_text()), Ok(Saved::default()));
}

#[test]
fn a_file_that_cannot_be_read_whole_is_not_read_at_all() {
    let good = two_tabs().to_text();
    for (bad, why) in [
        ("not = [toml", "not TOML"),
        ("version = 2\n", "another version"),
        ("current = 0\n", "no version"),
        (&good.replace("pane 2", "pane 7")[..], "a pane that is not there"),
        (&good.replace("\"pane 2\"", "\"pane 1\"")[..], "a pane twice"),
        (&good.replace("\"pane 2\"", "\"pane 2\", \"pane 0\"")[..], "more after the tree"),
        (&good.replace("row 2 5", "row 2 0")[..], "a share over nothing"),
        (&good.replace("row 2 5 34 60 40", "row 2 5 34 60")[..], "a share cut short"),
        (&good.replace("column", "diagonal")[..], "a split of no way"),
        (&good.replace("conversation = \"D9\"", "conversation = \"D9;rm\"")[..], "an id of another shape"),
        (&good.replace("conversation = \"D9\"", "conversation = \"\"")[..], "an empty id"),
        (&good.replace("thread = 1700000000000100", "thread = -1")[..], "a timestamp below zero"),
        (&good.replace("thread = 1700000000000100", "thread = \"x\"")[..], "a timestamp of words"),
        (&good.replace("active = 1", "active = -3")[..], "an index below zero"),
    ] {
        assert!(Saved::parse(bad).is_err(), "{why}: {bad}");
    }
    let many = format!(
        "version = 1\n{}",
        "[[tab]]\ntree = [\"pane 0\"]\n[[tab.pane]]\nworkspace = \"T\"\nconversation = \"C\"\n".repeat(MAX_TABS + 1)
    );
    assert!(Saved::parse(&many).is_err(), "too many tabs");
    let deep = format!("version = 1\n[[tab]]\ntree = [{}\"pane 0\"]\n", "\"row 1 2 1 1 1\", ".repeat(200));
    assert!(Saved::parse(&deep).is_err(), "a tree too deep");
}

#[test]
fn what_is_out_of_place_is_read_as_the_least_it_can_be() {
    let good = two_tabs().to_text();
    // An index past the end: the first.
    let got = Saved::parse(&good.replace("current = 1", "current = 9")).unwrap();
    assert_eq!(got.current, 0);
    let got = Saved::parse(&good.replace("active = 1", "active = 9")).unwrap();
    assert_eq!(got.tabs[0].active, PaneId(0));
    // A one-pane tab is never zoomed; a name loses control characters and length.
    let one = good.replacen("zoomed = false", "zoomed = true", 1);
    assert!(!Saved::parse(&one).unwrap().tabs[1].zoomed);
    let named = good.replace("name = \"Incident\"", &format!("name = \"\\u001b[2Jx{}\"", "y".repeat(300)));
    let name = Saved::parse(&named).unwrap().tabs[0].name.clone().unwrap();
    assert!(!name.contains('\u{1b}') && name.chars().count() <= MAX_NAME, "{name:?}");
    // A panel not beside its pane, or its own, is an ordinary pane.
    let far = good.replace("panel = 1", "panel = 2");
    assert_eq!(Saved::parse(&far).unwrap().tabs[0].panes[0].panel, None);
    let own = good.replace("panel = 1", "panel = 0");
    assert_eq!(Saved::parse(&own).unwrap().tabs[0].panes[0].panel, None);
}

#[test]
fn why_a_file_is_refused_quotes_only_the_start_of_a_long_word() {
    let long = "x".repeat(10_000);
    let good = two_tabs().to_text();
    for bad in [good.replace("column", &format!("column{long}")), format!("{long} = [")] {
        let why = Saved::parse(&bad).unwrap_err();
        assert!(why.chars().count() < 120, "{} chars: {}", why.len(), &why[..why.len().min(200)]);
        assert!(!why.contains('\n'), "one line: {why}");
    }
}

#[test]
fn a_tab_name_kept_is_the_name_read_back() {
    for given in ["  Incident  ", "a\u{1b}[2Jb", &"y".repeat(300), "\t", "  "] {
        let kept = tab_name(given);
        let mut saved = two_tabs();
        saved.tabs[0].name = kept.clone();
        assert_eq!(Saved::parse(&saved.to_text()).unwrap().tabs[0].name, kept, "{given:?}");
    }
    assert_eq!(tab_name("  Incident "), Some("Incident".to_string()));
    assert_eq!(tab_name(" \u{7} "), None);
}

/// The two panes of the first split whose sides are both panes.
fn two_leaves(n: &Node) -> Option<(PaneId, PaneId)> {
    match n {
        Node::Leaf(_) => None,
        Node::Split { first, second, .. } => match (&**first, &**second) {
            (Node::Leaf(a), Node::Leaf(b)) => Some((*a, *b)),
            _ => two_leaves(first).or_else(|| two_leaves(second)),
        },
    }
}

/// A tab of up to eight panes on two workspaces, splits either way with any share, a panel
/// beside some.
fn saved_tab() -> impl Strategy<Value = SavedTab> {
    let share = (0u16..300, 1u16..300, 0u16..100, any::<u16>(), 0u16..100)
        .prop_map(|(num, den, min, max, keep)| Share { num, den, min, max, keep });
    let steps = prop::collection::vec((0usize..8, any::<bool>(), share), 0..8);
    let targets = prop::collection::vec((any::<bool>(), 0u8..30, prop::option::of(0u64..u64::MAX / 2)), 9);
    (steps, targets, any::<bool>(), 0usize..9, prop::option::of("[a-zA-Z0-9 ·⤷#]{1,20}"), any::<bool>()).prop_map(
        |(steps, targets, zoomed, active, name, panel)| {
            let mut root = Node::Leaf(PaneId(0));
            for (k, (at, row, share)) in steps.into_iter().enumerate() {
                let leaves = root.leaves();
                let id = leaves[at % leaves.len()];
                let dir = if row { Dir::Row } else { Dir::Column };
                root = root.replace(id, &Node::split(dir, share, Node::Leaf(id), Node::Leaf(PaneId(k as u64 + 1))));
            }
            let n = root.leaves().len();
            let mut panes: Vec<SavedPane> = targets[..n]
                .iter()
                .map(|(a, c, ts)| {
                    let ws = if *a { "T1" } else { "T2" };
                    let c = format!("C{c}");
                    let target = ts.map_or_else(|| conv(ws, &c), |ts| thread(ws, &c, ts));
                    SavedPane { target, panel: None }
                })
                .collect();
            // The last split's sides, when both are panes, may be a pane and its panel.
            if panel && let Some((a, b)) = two_leaves(&root) {
                panes[a.0 as usize].panel = Some(b);
            }
            let zoomed = zoomed && n > 1;
            let name = name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            SavedTab { name, root, active: PaneId((active % n) as u64), zoomed, panes }
        },
    )
}

proptest! {
    /// Whatever the tabs, written and read back they are the same.
    #[test]
    fn written_then_read_is_the_same(tabs in prop::collection::vec(saved_tab(), 0..5), current in 0usize..5) {
        let current = if current < tabs.len() { current } else { 0 };
        let saved = Saved { tabs, current };
        prop_assert_eq!(Saved::parse(&saved.to_text()), Ok(saved));
    }

    /// Any text is read or refused, never a panic.
    #[test]
    fn any_text_is_read_or_refused(text in ".{0,400}") {
        let _ = Saved::parse(&text);
        let _ = Saved::parse(&format!("version = 1\n[[tab]]\ntree = [\"{text}\"]\n"));
    }
}
