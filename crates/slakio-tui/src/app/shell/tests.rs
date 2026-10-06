use super::*;
use crate::app::model::Model;
use slakio_world::World;

fn model() -> Model {
    Model::new(World::demo().snapshot().clone())
}

#[test]
fn focus_moves_between_the_regions_and_skips_a_hidden_list() {
    let m = model();
    let mut s = Shell::default();
    let mut f = Region::List;
    assert_eq!(f, Region::List);
    s.update(ShellAction::FocusLeft, &m, 10, &mut f);
    assert_eq!(f, Region::Nav);
    s.update(ShellAction::FocusLeft, &m, 10, &mut f);
    assert_eq!(f, Region::Nav, "stays at the edge");
    s.update(ShellAction::FocusRight, &m, 10, &mut f);
    s.update(ShellAction::FocusRight, &m, 10, &mut f);
    assert_eq!(f, Region::Work);
    s.update(ShellAction::ToggleList, &m, 10, &mut f);
    assert!(s.list_hidden);
    s.update(ShellAction::FocusLeft, &m, 10, &mut f);
    assert_eq!(f, Region::Nav, "the hidden list is skipped");
}

#[test]
fn hiding_the_focused_list_moves_the_focus_to_the_work_area() {
    let m = model();
    let mut s = Shell::default();
    let mut f = Region::List;
    s.update(ShellAction::ToggleList, &m, 10, &mut f);
    assert_eq!(f, Region::Work);
    s.update(ShellAction::Show(View::Dms), &m, 10, &mut f);
    assert!(!s.list_hidden, "showing a view shows the list again");
}

#[test]
fn the_top_bar_selects_views_and_the_switcher_workspaces() {
    let m = model();
    let mut s = Shell::default();
    let mut f = Region::List;
    s.update(ShellAction::FocusLeft, &m, 10, &mut f);
    assert_eq!(nav_items()[s.nav_cursor], NavItem::View(View::Home), "on the view shown");
    s.update(ShellAction::NavPrev, &m, 10, &mut f);
    assert_eq!(nav_items()[s.nav_cursor], NavItem::Workspace);
    s.update(ShellAction::NavSelect, &m, 10, &mut f);
    assert_eq!(f, Region::Nav, "the workspace is picked in the switcher (the app's)");
    s.update(ShellAction::NavNext, &m, 10, &mut f);
    s.update(ShellAction::NavNext, &m, 10, &mut f);
    s.update(ShellAction::NavSelect, &m, 10, &mut f);
    assert_eq!((s.workspace, s.view, f), (0, View::Dms, Region::List));
    s.update(ShellAction::FocusLeft, &m, 10, &mut f);
    for _ in 0..20 {
        s.update(ShellAction::NavNext, &m, 10, &mut f);
    }
    assert_eq!(nav_items()[s.nav_cursor], NavItem::View(View::Later), "stops at the last item");
    s.select_workspace(1, &m);
    assert_eq!((s.workspace, s.view), (1, View::Home));
    s.update(ShellAction::Show(View::Dms), &m, 10, &mut f);
    assert_eq!(nav_items()[s.nav_cursor], NavItem::View(View::Dms), "the top bar follows");
    assert!(View::Later.is_placeholder() && !View::Dms.is_placeholder());
}

#[test]
fn the_list_cursor_stays_in_range_and_on_screen() {
    let m = model();
    let mut s = Shell::default();
    let mut f = Region::List;
    let rows = s.rows(&m).len();
    s.update(ShellAction::ListPrev, &m, 10, &mut f);
    assert_eq!(s.list_cursor, 0);
    s.update(ShellAction::ListLast, &m, 10, &mut f);
    assert_eq!(s.list_cursor, rows - 1);
    assert_eq!(s.list_top, rows - 10, "the last row is at the bottom");
    s.update(ShellAction::ListNext, &m, 10, &mut f);
    assert_eq!(s.list_cursor, rows - 1);
    s.update(ShellAction::ListFirst, &m, 10, &mut f);
    assert_eq!((s.list_cursor, s.list_top), (0, 0));
    let list = s.rows(&m);
    for _ in 0..12 {
        s.update(ShellAction::ListNext, &m, 10, &mut f);
        assert!(list[s.list_cursor].is_selectable(), "never on the blank row between sections");
    }
    // Twelve rows down, past the two blank rows on the way.
    assert_eq!((s.list_cursor, s.list_top), (14, 5));
    s.update(ShellAction::ListPageUp, &m, 10, &mut f);
    assert_eq!(s.list_cursor, 4);
    s.update(ShellAction::ListHalfDown, &m, 10, &mut f);
    assert!(s.list_cursor > 4 && list[s.list_cursor].is_selectable());
}

#[test]
fn enter_folds_a_section_and_opens_a_conversation() {
    let m = model();
    let mut s = Shell::default();
    let mut f = Region::List;
    let before = s.rows(&m).len();
    s.list_cursor = 0;
    s.update(ShellAction::ListOpen, &m, 10, &mut f);
    assert_eq!(s.rows(&m).len(), before - 2, "Favorites folded");
    s.update(ShellAction::ListOpen, &m, 10, &mut f);
    assert_eq!(s.rows(&m).len(), before, "and unfolded");
    s.update(ShellAction::ListNext, &m, 10, &mut f);
    let open = s.update(ShellAction::ListOpen, &m, 10, &mut f).expect("a conversation to open");
    assert!(open.focus);
    assert_eq!(m.target(&open.target).unwrap().name, "backend");
    let peek = s.update(ShellAction::ListPeek, &m, 10, &mut f).expect("a peek");
    assert!(!peek.focus);
    assert_eq!(f, Region::List, "the app moves the focus, not the shell");
}

#[test]
fn clamping_after_a_smaller_model_keeps_cursors_valid() {
    let m = model();
    let mut s = Shell::default();
    let mut f = Region::List;
    s.update(ShellAction::ListLast, &m, 10, &mut f);
    s.workspace = 1;
    s.clamp(&Model::default(), 10);
    assert_eq!((s.workspace, s.list_cursor), (0, 0));
}
