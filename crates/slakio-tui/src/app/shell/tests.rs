use super::*;
use crate::app::model::Model;
use slakio_world::World;

fn model() -> Model {
    Model::new(World::demo().snapshot().clone())
}

#[test]
fn focus_moves_between_the_regions_and_skips_a_hidden_list_with_its_view_switcher() {
    let m = model();
    let mut s = Shell::default();
    let mut f = Region::List;
    s.update(ShellAction::FocusLeft, &m, 10, &mut f);
    assert_eq!(f, Region::ViewSwitcher);
    s.update(ShellAction::FocusLeft, &m, 10, &mut f);
    assert_eq!(f, Region::ViewSwitcher, "stays at the edge");
    s.update(ShellAction::FocusRight, &m, 10, &mut f);
    s.update(ShellAction::FocusRight, &m, 10, &mut f);
    assert_eq!(f, Region::Work);
    s.update(ShellAction::ToggleList, &m, 10, &mut f);
    assert!(s.list_hidden);
    s.update(ShellAction::FocusLeft, &m, 10, &mut f);
    assert_eq!(f, Region::Work, "the hidden list and its view switcher are skipped");
    s.update(ShellAction::FocusNav, &m, 10, &mut f);
    assert_eq!((f, s.list_hidden), (Region::ViewSwitcher, false), "the view switcher shows the list again");
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
fn the_view_switcher_and_the_brackets_select_views() {
    let m = model();
    let mut s = Shell::default();
    let mut f = Region::List;
    s.update(ShellAction::FocusNav, &m, 10, &mut f);
    assert_eq!((f, View::ALL[s.nav_cursor]), (Region::ViewSwitcher, View::Home), "on the view shown");
    s.update(ShellAction::NavPrev, &m, 10, &mut f);
    assert_eq!(s.nav_cursor, 0, "Home is the first");
    s.update(ShellAction::NavNext, &m, 10, &mut f);
    s.update(ShellAction::NavSelect, &m, 10, &mut f);
    assert_eq!((s.workspace, s.view, f), (0, View::Dms, Region::List));
    s.update(ShellAction::FocusNav, &m, 10, &mut f);
    for _ in 0..20 {
        s.update(ShellAction::NavNext, &m, 10, &mut f);
    }
    assert_eq!(View::ALL[s.nav_cursor], View::Later, "stops at the last view");
    s.update(ShellAction::NavLeave, &m, 10, &mut f);
    assert_eq!((f, s.view), (Region::List, View::Dms), "back, showing nothing new");
    s.update(ShellAction::ViewNext, &m, 10, &mut f);
    assert_eq!((s.view, View::ALL[s.nav_cursor]), (View::Activity, View::Activity));
    for _ in 0..3 {
        s.update(ShellAction::ViewPrev, &m, 10, &mut f);
    }
    assert_eq!(s.view, View::Later, "round again");
    s.update(ShellAction::ViewNext, &m, 10, &mut f);
    assert_eq!(s.view, View::Home);
    s.select_workspace(1, &m);
    assert_eq!((s.workspace, s.view, s.nav_cursor), (1, View::Home, 0));
    s.update(ShellAction::Show(View::Dms), &m, 10, &mut f);
    assert_eq!(View::ALL[s.nav_cursor], View::Dms, "the view switcher follows");
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
