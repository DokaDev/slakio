use super::*;
use slakio_core::model::Org;
use slakio_world::World;

fn model() -> Model {
    Model::new(World::demo().snapshot().clone())
}

/// The rows of `view`, sections in `collapsed` folded, no group folded, To-dos unfiltered.
fn rows_of(m: &Model, ws: usize, view: View, collapsed: &HashSet<SectionId>) -> Vec<Row> {
    let ws = m.workspaces().get(ws).map_or_else(|| WorkspaceId::new("T-none"), |w| w.id.clone());
    m.rows(&ws, view, collapsed, &HashSet::new(), &TodoView::default())
}

/// The id of workspace `i`.
fn ws(m: &Model, i: usize) -> WorkspaceId {
    m.workspaces()[i].id.clone()
}

#[test]
fn home_lists_each_section_then_its_conversations_and_folds_collapsed_ones() {
    let m = model();
    let rows = rows_of(&m, 0, View::Home, &HashSet::new());
    assert!(matches!(rows[0], Row::Section(_)));
    let headers: Vec<&str> = rows
        .iter()
        .filter_map(|r| if let Row::Section(i) = r { Some(m.section(*i).name.unsanitized()) } else { None })
        .collect();
    assert_eq!(headers, ["Favorites", "Ops", "Channels", "Direct messages"]);
    let Row::Section(fav) = rows[0] else { unreachable!() };
    let folded = rows_of(&m, 0, View::Home, &HashSet::from([m.section(fav).id.clone()]));
    assert_eq!(folded[1], Row::Spacer, "the folded section shows only its header");
    assert!(matches!(folded[2], Row::Section(_)));
    assert_eq!(rows.len() - folded.len(), 2, "Favorites holds two channels");
    // One blank row between two sections, none before the first or after the last.
    let spacers: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| **r == Row::Spacer).map(|(i, _)| i).collect();
    // Saved, the last group, after a blank row too.
    assert_eq!(spacers.len(), headers.len());
    assert_eq!(rows[spacers[spacers.len() - 1] + 1], Row::Group(Group::Saved));
    for i in spacers {
        assert!(rows[i + 1].is_header() && !rows[i].is_selectable(), "row {i}");
    }
}

#[test]
fn dms_lists_only_the_dms_of_the_workspace_and_unbuilt_views_are_empty() {
    let m = model();
    let dms = rows_of(&m, 1, View::Dms, &HashSet::new());
    assert!(!dms.is_empty());
    for r in &dms {
        let Row::Conversation(i) = r else { panic!("{r:?}") };
        assert!(m.conversation(*i).is_dm());
        assert_eq!(m.conversation(*i).workspace, m.workspaces()[1].id);
    }
    for v in [View::Activity, View::Files] {
        assert!(rows_of(&m, 0, v, &HashSet::new()).is_empty());
    }
    assert!(rows_of(&m, 9, View::Home, &HashSet::new()).is_empty(), "no such workspace");
}

#[test]
fn counts_add_up() {
    let m = model();
    assert!(m.mentions() > 0);
    assert!(m.dm_unread(&ws(&m, 0)) > 0 || m.dm_unread(&ws(&m, 1)) > 0);
    assert!(m.workspace_unread(&ws(&m, 0)));
    let sections = rows_of(&m, 0, View::Home, &HashSet::new());
    let (unread, mentions) = sections
        .iter()
        .filter_map(|r| if let Row::Section(i) = r { Some(m.section_counts(*i)) } else { None })
        .fold((0, 0), |(a, b), (u, n)| (a + u, b + n));
    assert!(unread > 0);
    assert_eq!(mentions, m.workspace_mentions(&ws(&m, 0)));
    assert!(!Model::default().is_loaded() && m.is_loaded());
}

/// Workspace `i`'s part of the demo world, as its own boot answers it.
fn part(i: usize) -> (WorkspaceId, Snapshot) {
    let s = World::demo().snapshot().clone();
    let id = s.workspaces[i].id.clone();
    let only = s.only(&id);
    (id, only)
}

fn signed(m: &Model) -> Vec<SignedIn> {
    m.workspaces().iter().map(|w| SignedIn { id: w.id.clone(), name: "n".into(), read_only: true }).collect()
}

#[test]
fn a_person_listed_by_two_workspaces_is_kept_as_each_one_sees_them() {
    let (a, b) = (WorkspaceId::new("T1"), WorkspaceId::new("T2"));
    let peer = |ws: &WorkspaceId, org: Org| User {
        id: UserId::new("U-peer"),
        workspace: ws.clone(),
        name: Remote::new("peer"),
        display_name: Remote::new("Peer"),
        org,
        bot: false,
        presence: slakio_core::model::Presence::Unknown,
    };
    let mut m = Model::default();
    m.add_users(&[peer(&a, Org::Own), peer(&b, Org::External(Remote::new("A corp")))]);
    let id = UserId::new("U-peer");
    assert_eq!(m.user(&a, &id).map(|u| &u.org), Some(&Org::Own));
    assert_eq!(m.user(&b, &id).map(|u| &u.org), Some(&Org::External(Remote::new("A corp"))), "not overwritten by A's");
    // Learned again from a page of B: B's entry changes, A's does not.
    m.add_users(&[User { presence: slakio_core::model::Presence::Away, ..peer(&b, Org::Own) }]);
    assert_eq!(m.user(&a, &id).map(|u| u.presence), Some(slakio_core::model::Presence::Unknown));
    assert_eq!(m.user(&b, &id).map(|u| u.presence), Some(slakio_core::model::Presence::Away));
    // A workspace that does not list them yet shows their name all the same.
    assert_eq!(m.user(&WorkspaceId::new("T3"), &id).map(|u| u.display_name.unsanitized()), Some("Peer"));
    assert!(m.user(&a, &UserId::new("U-nobody")).is_none());
}

#[test]
fn one_workspaces_boot_replaces_its_part_and_leaves_the_others_alone() {
    let whole = model();
    let (a, part_a) = part(0);
    let (b, part_b) = part(1);
    let mut m = Model::default();
    m.sign_ins(&signed(&whole));
    assert_eq!(m.workspaces().len(), 2, "listed before either answers");
    assert!(!m.is_loaded_in(&a) && !m.is_loaded_in(&b) && !m.is_loaded());
    m.merge(&b, &part_b, false);
    assert!(m.is_loaded_in(&b) && !m.is_loaded_in(&a), "B answered first; A still loading");
    assert!(rows_of(&m, 0, View::Home, &HashSet::new()).is_empty(), "nothing of A yet");
    m.merge(&a, &part_a, false);
    for i in 0..2 {
        let mut got: Vec<String> =
            m.conversations().iter().filter(|c| c.workspace == ws(&m, i)).map(|c| c.id.to_string()).collect();
        let mut want: Vec<String> =
            whole.conversations().iter().filter(|c| c.workspace == ws(&whole, i)).map(|c| c.id.to_string()).collect();
        got.sort();
        want.sort();
        assert_eq!(got, want, "workspace {i}");
    }
    // A's boot again, with one conversation fewer: B keeps everything.
    let mut smaller = part_a.clone();
    smaller.conversations.pop();
    let b_before: Vec<_> = m.conversations().iter().filter(|c| c.workspace == b).cloned().collect();
    m.merge(&a, &smaller, false);
    assert_eq!(m.conversations().iter().filter(|c| c.workspace == a).count(), part_a.conversations.len() - 1);
    assert_eq!(m.conversations().iter().filter(|c| c.workspace == b).cloned().collect::<Vec<_>>(), b_before);
    assert_eq!(m.workspaces()[0].id, a, "the order is the sign-in's, not the answers'");
}

#[test]
fn a_boot_answer_brings_nothing_of_another_workspace() {
    let whole = model();
    let (a, _) = part(0);
    let b = ws(&whole, 1);
    let mut m = Model::default();
    m.sign_ins(&signed(&whole));
    // A backend for A that answers with every workspace: only A's part is taken.
    m.merge(&a, whole_snapshot(), false);
    assert!(!m.is_loaded_in(&b));
    assert!(m.conversations().iter().all(|c| c.workspace == a));
    assert!(m.workspaces().iter().any(|w| w.id == b), "B is still listed, loading");
}

fn whole_snapshot() -> &'static Snapshot {
    static S: std::sync::OnceLock<Snapshot> = std::sync::OnceLock::new();
    S.get_or_init(|| World::demo().snapshot().clone())
}

#[test]
fn a_cached_boot_shows_until_the_backends_own_and_never_after_it() {
    let (a, part_a) = part(0);
    let mut m = Model::default();
    m.sign_ins(&[SignedIn { id: a.clone(), name: "A".into(), read_only: true }]);
    m.merge(&a, &part_a, true);
    assert!(m.is_cached_in(&a) && m.is_loaded_in(&a) && !m.is_fresh(&a));
    m.merge(&a, &part_a, false);
    assert!(!m.is_cached_in(&a) && m.is_fresh(&a));
}

#[test]
fn signing_out_of_a_workspace_drops_its_part_and_keeps_the_colors_by_place() {
    let whole = model();
    let mut m = whole.clone();
    let b = signed(&whole).pop().unwrap();
    m.sign_ins(std::slice::from_ref(&b));
    assert_eq!(m.workspaces().iter().map(|w| &w.id).collect::<Vec<_>>(), vec![&b.id]);
    assert_eq!(m.workspaces()[0].color, WorkspaceColor(0), "the first place's color");
    assert!(m.conversations().iter().all(|c| c.workspace == b.id));
    assert!(m.is_loaded_in(&b.id), "what B had is kept");
    // A placeholder has no "me" yet: nothing is sent as nobody.
    let mut fresh = Model::default();
    fresh.sign_ins(&signed(&whole));
    assert_eq!(fresh.me(&ws(&whole, 0)), None);
    assert!(!fresh.saved_known(&ws(&whole, 0)), "unknown until the boot says");
}

#[test]
fn a_row_id_outlives_its_index() {
    let m = model();
    let rows = rows_of(&m, 0, View::Home, &HashSet::new());
    let Some(at) = rows.iter().position(|r| matches!(r, Row::Conversation(_))) else { panic!("a conversation") };
    let key = m.key(rows[at]).unwrap();
    assert_eq!(m.key(Row::Spacer), None);
    assert!(matches!(key, RowKey::Conversation(_)));
}

mod properties {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Whatever order the workspaces' boots answer in, and whatever each one drops, every
        /// workspace shows exactly its last answer, and people stay keyed by their workspace.
        #[test]
        fn every_workspace_shows_its_own_last_answer(steps in proptest::collection::vec((0usize..2, any::<u64>()), 1..12)) {
            let whole = model();
            let mut m = Model::default();
            m.sign_ins(&signed(&whole));
            let mut last: [Option<Snapshot>; 2] = [None, None];
            for (i, mask) in steps {
                let (id, mut s) = part(i);
                let mut bit = 0;
                s.conversations.retain(|_| { bit += 1; mask >> (bit % 64) & 1 == 1 });
                m.merge(&id, &s, false);
                last[i] = Some(s);
            }
            for (i, want) in last.iter().enumerate() {
                let id = ws(&m, i);
                let got: Vec<_> = m.conversations().iter().filter(|c| c.workspace == id).cloned().collect();
                match want {
                    Some(s) => {
                        prop_assert_eq!(&got, &s.conversations);
                        for u in &s.users {
                            prop_assert_eq!(m.user(&id, &u.id), Some(u));
                        }
                    }
                    None => prop_assert!(got.is_empty() && !m.is_loaded_in(&id)),
                }
            }
        }

        /// The cursor stays on its row's id while the other workspace answers, or its own
        /// answer adds rows before it.
        #[test]
        fn the_cursor_follows_its_row_through_answers(pick in 0usize..40, drop_first in any::<bool>()) {
            let (a, part_a) = part(0);
            let (b, part_b) = part(1);
            let mut m = Model::default();
            m.sign_ins(&[
                SignedIn { id: a.clone(), name: "A".into(), read_only: true },
                SignedIn { id: b.clone(), name: "B".into(), read_only: true },
            ]);
            let mut first = part_a.clone();
            if drop_first {
                first.conversations.remove(0);
            }
            m.merge(&a, &first, false);
            let mut shell = super::super::super::shell::Shell::default();
            let rows = shell.rows(&m);
            let selectable: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].is_selectable()).collect();
            shell.list_cursor = selectable[pick % selectable.len()];
            let anchor = shell.anchor(&m);
            for (id, s) in [(&b, &part_b), (&a, &part_a)] {
                m.merge(id, s, false);
                shell.restore(anchor.clone(), &m, 10);
                prop_assert_eq!(shell.anchor(&m), anchor.clone());
            }
        }
    }
}
