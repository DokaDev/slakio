use super::*;
use slakio_core::model::{Org, SavedItem, SavedState};
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
    m.workspaces()
        .iter()
        .map(|w| SignedIn {
            id: w.id.clone(),
            name: "n".into(),
            read_only: true,
            host: String::new(),
            user: String::new(),
        })
        .collect()
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
        avatar: None,
        no_photo: false,
    };
    let mut m = Model::default();
    m.add_users(&a, &[peer(&a, Org::Own)]);
    m.add_users(&b, &[peer(&b, Org::External(Remote::new("A corp")))]);
    let id = UserId::new("U-peer");
    assert_eq!(m.user(&a, &id).map(|u| &u.org), Some(&Org::Own));
    assert_eq!(m.user(&b, &id).map(|u| &u.org), Some(&Org::External(Remote::new("A corp"))), "not overwritten by A's");
    // Learned again from a page of B: B's entry changes, A's does not.
    m.add_users(&b, &[User { presence: slakio_core::model::Presence::Away, ..peer(&b, Org::Own) }]);
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
    m.sign_ins(&[SignedIn {
        id: a.clone(),
        name: "A".into(),
        read_only: true,
        host: String::new(),
        user: String::new(),
    }]);
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
                SignedIn { id: a.clone(), name: "A".into(), read_only: true, host: String::new(), user: String::new() },
                SignedIn { id: b.clone(), name: "B".into(), read_only: true, host: String::new(), user: String::new() },
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

#[test]
fn an_answer_of_one_workspace_never_changes_another_workspaces_people_or_saved_items() {
    let mut m = model();
    let (a, b) = (ws(&m, 0), ws(&m, 1));
    let all: Vec<SavedItem> = m.snapshot.saved.clone().unwrap();
    let of =
        |items: &[SavedItem], w: &WorkspaceId| items.iter().filter(|i| &i.workspace == w).cloned().collect::<Vec<_>>();
    assert!(!of(&all, &a).is_empty() && !of(&all, &b).is_empty(), "both workspaces have saved items");
    // A's answer: one of A's items gone, and an item tagged with B's workspace in it.
    let mut answer = of(&all, &a)[1..].to_vec();
    let mut stray = of(&all, &b)[0].clone();
    stray.state = SavedState::Completed;
    answer.push(stray);
    m.set_saved_in(&a, answer, m.taken());
    let now = m.snapshot.saved.clone().unwrap();
    assert_eq!(of(&now, &a), of(&all, &a)[1..].to_vec(), "A's items as its answer says");
    assert_eq!(of(&now, &b), of(&all, &b), "B's items are not touched");
    // A page of A that names a person of B: B's view of them is not changed.
    let id = m.snapshot.users.iter().find(|u| u.workspace == b).unwrap().id.clone();
    let before = m.user(&b, &id).cloned();
    let changed = User { display_name: Remote::new("Changed"), ..before.clone().unwrap() };
    m.add_users(&a, &[changed]);
    assert_eq!(m.user(&b, &id).cloned(), before);
}

/// A bot that posts under ever new names keeps [`BOT_NAMES`] of them as people, the newest;
/// people stay as they are.
#[test]
fn a_bot_s_names_kept_as_people_are_capped() {
    let mut m = model();
    let w = ws(&m, 0);
    let before = m.snapshot.users.len();
    let bot = |i: usize| slakio_core::model::User {
        id: UserId::new(format!("BHOOK~{i:016x}")),
        workspace: w.clone(),
        name: slakio_core::sanitize::Remote::new(format!("name {i}")),
        display_name: slakio_core::sanitize::Remote::new(format!("name {i}")),
        org: slakio_core::model::Org::Own,
        bot: true,
        presence: slakio_core::model::Presence::Unknown,
        avatar: None,
        no_photo: false,
    };
    for i in 0..1000 {
        m.add_users(&w, &[bot(i)]);
    }
    assert_eq!(m.snapshot.users.len(), before + BOT_NAMES);
    assert!(m.user(&w, &bot(999).id).is_some() && m.user(&w, &bot(1000 - BOT_NAMES).id).is_some());
    assert!(m.user(&w, &bot(0).id).is_none(), "the oldest name went");
    let named = m.user(&w, &bot(990).id).unwrap();
    assert_eq!(named.display_name.line().as_str(), "name 990", "each id still names its own");
}

#[test]
fn a_known_photo_is_never_replaced_by_no_photo_but_a_new_photo_replaces_it() {
    let a = WorkspaceId::new("T1");
    let id = UserId::new("U-peer");
    let peer = |avatar: Option<&str>| User {
        id: id.clone(),
        workspace: a.clone(),
        name: Remote::new("peer"),
        display_name: Remote::new("Peer"),
        org: Org::Own,
        bot: false,
        presence: slakio_core::model::Presence::Unknown,
        avatar: avatar.map(str::to_string),
        no_photo: false,
    };
    let host = slakio_core::images::AVATAR_HOSTS[0];
    let (one, two) = (format!("https://{host}/one_72.png"), format!("https://{host}/two_72.png"));
    let (one, two) = (one.as_str(), two.as_str());
    let mut m = Model::default();
    m.add_users(&a, &[peer(Some(one))]);
    m.add_users(&a, &[User { presence: slakio_core::model::Presence::Away, ..peer(None) }]);
    let got = m.user(&a, &id).unwrap();
    assert_eq!(got.avatar.as_deref(), Some(one), "a later page without a photo keeps it");
    assert_eq!(got.presence, slakio_core::model::Presence::Away, "the rest of the record is the new one");
    m.add_users(&a, &[peer(Some(two))]);
    assert_eq!(m.user(&a, &id).unwrap().avatar.as_deref(), Some(two), "a real new photo replaces it");
}

#[test]
fn a_boot_answer_without_a_photo_keeps_the_photo_shown() {
    let (a, part_a) = part(0);
    let mut m = Model::default();
    m.sign_ins(&signed(&model())[..1]);
    let host = slakio_core::images::AVATAR_HOSTS[0];
    let photo = &format!("https://{host}/me_72.png");
    let mut with = part_a.clone();
    let me = with.workspaces[0].me.clone();
    for u in with.users.iter_mut().filter(|u| u.id == me) {
        u.avatar = Some(photo.clone());
    }
    m.merge(&a, &with, false);
    assert_eq!(m.user(&a, &me).unwrap().avatar.as_deref(), Some(photo.as_str()));
    let mut without = part_a.clone();
    for u in &mut without.users {
        u.avatar = None;
    }
    m.merge(&a, &without, false);
    assert_eq!(m.user(&a, &me).unwrap().avatar.as_deref(), Some(photo.as_str()), "a re-boot without it keeps it");
    let other = format!("https://{host}/new_72.png");
    for u in without.users.iter_mut().filter(|u| u.id == me) {
        u.avatar = Some(other.clone());
    }
    m.merge(&a, &without, false);
    assert_eq!(m.user(&a, &me).unwrap().avatar.as_deref(), Some(other.as_str()), "a real new photo replaces it");
}

/// A record that says the person has no photo of their own drops the photo known, as a page
/// brings it, as a workspace's boot does and as a whole new boot does.
#[test]
fn a_record_saying_no_photo_of_their_own_drops_the_photo_shown() {
    let (a, part_a) = part(0);
    let host = slakio_core::images::AVATAR_HOSTS[0];
    let photo = format!("https://{host}/me_72.png");
    let me = part_a.workspaces[0].me.clone();
    let with = |avatar: Option<&String>, no_photo: bool| {
        let mut s = part_a.clone();
        for u in s.users.iter_mut().filter(|u| u.id == me) {
            (u.avatar, u.no_photo) = (avatar.cloned(), no_photo);
        }
        s
    };
    let mut m = Model::default();
    m.sign_ins(&signed(&model())[..1]);
    m.merge(&a, &with(Some(&photo), false), false);
    let gone = User { avatar: None, no_photo: true, ..m.user(&a, &me).unwrap().clone() };
    m.add_users(&a, &[gone]);
    assert_eq!(m.user(&a, &me).unwrap().avatar, None, "a page's record");
    m.merge(&a, &with(Some(&photo), false), false);
    m.merge(&a, &with(None, true), false);
    assert_eq!(m.user(&a, &me).unwrap().avatar, None, "a workspace's boot");
    m.merge(&a, &with(Some(&photo), false), false);
    let next = m.with_photos(with(None, true));
    assert_eq!(next.users.iter().find(|u| u.id == me).unwrap().avatar, None, "a whole new boot");
    let next = m.with_photos(with(None, false));
    assert_eq!(next.users.iter().find(|u| u.id == me).unwrap().avatar.as_ref(), Some(&photo), "not said: kept");
}

#[test]
fn empty_starred_external_and_apps_sections_are_not_shown_the_others_are() {
    let mut s = World::demo().snapshot().clone();
    let ws = s.workspaces[0].id.clone();
    let mut add = |name: &str, kind: SectionKind| {
        let id = SectionId::new(format!("empty-{name}"));
        s.sections.push(Section { id, workspace: ws.clone(), name: Remote::new(name), kind });
    };
    for (name, kind) in [
        ("Empty starred", SectionKind::Favorites),
        ("Empty external", SectionKind::External),
        ("Empty apps", SectionKind::Apps),
        ("Empty custom", SectionKind::Custom),
        ("Empty channels", SectionKind::Channels),
        ("Empty DMs", SectionKind::DirectMessages),
    ] {
        add(name, kind);
    }
    // A filled Apps section shows.
    let dm = s.conversations.iter().position(|c| c.workspace == ws && c.is_dm()).unwrap();
    add("Apps", SectionKind::Apps);
    s.conversations[dm].section = SectionId::new("empty-Apps");
    let m = Model::new(s);
    let rows = rows_of(&m, 0, View::Home, &HashSet::new());
    let headers: Vec<&str> = rows
        .iter()
        .filter_map(|r| if let Row::Section(i) = r { Some(m.section(*i).name.unsanitized()) } else { None })
        .collect();
    assert_eq!(
        headers,
        ["Favorites", "Ops", "Channels", "Direct messages", "Empty custom", "Empty channels", "Empty DMs", "Apps"]
    );
    assert!(!rows.windows(2).any(|w| w[0] == Row::Spacer && w[1] == Row::Spacer), "no blank row doubled");
}

/// A photo replaced is noted as gone once, unless someone still shows it (a bot's names share
/// its picture); a whole new boot notes those it shows for no one.
#[test]
fn a_photo_no_one_shows_any_more_is_noted_as_gone() {
    let mut m = model();
    let w = ws(&m, 0);
    let host = slakio_core::images::AVATAR_HOSTS[0];
    let icon = format!("https://{host}/bot_72.png");
    let bot = |i: usize, avatar: &str| User {
        id: UserId::new(format!("BHOOK~{i:016x}")),
        workspace: w.clone(),
        name: Remote::new(format!("name {i}")),
        display_name: Remote::new(format!("name {i}")),
        org: Org::Own,
        bot: true,
        presence: slakio_core::model::Presence::Unknown,
        avatar: Some(avatar.to_string()),
        no_photo: false,
    };
    m.add_users(&w, &[bot(1, &icon), bot(2, &icon)]);
    let _ = m.take_gone_photos();
    let other = format!("https://{host}/bot2_72.png");
    m.add_users(&w, &[bot(1, &other)]);
    assert!(m.take_gone_photos().is_empty(), "the other name still shows it");
    m.add_users(&w, &[bot(2, &other), bot(2, &other)]);
    assert_eq!(m.take_gone_photos(), std::slice::from_ref(&icon), "once, now that no one shows it");
    let next = m.renewed(m.snapshot.clone());
    let mut fewer = m.snapshot.clone();
    fewer.users.retain(|u| u.avatar.as_ref() != Some(&other));
    let mut next2 = next.renewed(fewer);
    assert_eq!(next2.take_gone_photos(), std::slice::from_ref(&other), "a whole new boot that shows it for no one");
    // A workspace's boot that gives its user a new photo.
    let (a, part_a) = part(0);
    let me = part_a.workspaces[0].me.clone();
    let with = |photo: &str| {
        let mut s = part_a.clone();
        for u in s.users.iter_mut().filter(|u| u.id == me) {
            u.avatar = Some(photo.to_string());
        }
        s
    };
    let mut m = Model::default();
    m.sign_ins(&signed(&model())[..1]);
    m.merge(&a, &with(&icon), false);
    let _ = m.take_gone_photos();
    m.merge(&a, &with(&other), false);
    assert_eq!(m.take_gone_photos(), [icon], "a workspace's boot");
}
