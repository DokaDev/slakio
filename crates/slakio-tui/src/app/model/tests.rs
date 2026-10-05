use super::*;
use slakio_world::World;

fn model() -> Model {
    Model::new(World::demo().snapshot().clone())
}

#[test]
fn home_lists_each_section_then_its_conversations_and_folds_collapsed_ones() {
    let m = model();
    let rows = m.rows(0, View::Home, &HashSet::new());
    assert!(matches!(rows[0], Row::Section(_)));
    let headers: Vec<&str> = rows
        .iter()
        .filter_map(|r| if let Row::Section(i) = r { Some(m.section(*i).name.unsanitized()) } else { None })
        .collect();
    assert_eq!(headers, ["Favorites", "Ops", "Channels", "Direct messages"]);
    let Row::Section(fav) = rows[0] else { unreachable!() };
    let folded = m.rows(0, View::Home, &HashSet::from([m.section(fav).id.clone()]));
    assert_eq!(folded[1], Row::Spacer, "the folded section shows only its header");
    assert!(matches!(folded[2], Row::Section(_)));
    assert_eq!(rows.len() - folded.len(), 2, "Favorites holds two channels");
    // One blank row between two sections, none before the first or after the last.
    let spacers: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| **r == Row::Spacer).map(|(i, _)| i).collect();
    assert_eq!(spacers.len(), headers.len() - 1);
    for i in spacers {
        assert!(matches!(rows[i + 1], Row::Section(_)) && !rows[i].is_selectable(), "row {i}");
    }
}

#[test]
fn dms_lists_only_the_dms_of_the_workspace_and_unbuilt_views_are_empty() {
    let m = model();
    let dms = m.rows(1, View::Dms, &HashSet::new());
    assert!(!dms.is_empty());
    for r in &dms {
        let Row::Conversation(i) = r else { panic!("{r:?}") };
        assert!(m.conversation(*i).is_dm());
        assert_eq!(m.conversation(*i).workspace, m.workspaces()[1].id);
    }
    for v in [View::Activity, View::Files, View::Later] {
        assert!(m.rows(0, v, &HashSet::new()).is_empty());
    }
    assert!(m.rows(9, View::Home, &HashSet::new()).is_empty(), "no such workspace");
}

#[test]
fn counts_add_up() {
    let m = model();
    assert!(m.mentions() > 0);
    assert!(m.dm_unread(0) > 0 || m.dm_unread(1) > 0);
    assert!(m.workspace_unread(0));
    let sections = m.rows(0, View::Home, &HashSet::new());
    let (unread, mentions) = sections
        .iter()
        .filter_map(|r| if let Row::Section(i) = r { Some(m.section_counts(*i)) } else { None })
        .fold((0, 0), |(a, b), (u, n)| (a + u, b + n));
    assert!(unread > 0);
    assert_eq!(mentions, m.workspace_mentions(0));
    assert!(!Model::default().is_loaded() && m.is_loaded());
}
