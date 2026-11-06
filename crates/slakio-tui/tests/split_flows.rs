//! Splits over the demo world, driven headless: `v` opens beside the pane and `s` under it (the
//! selected message's thread, else the same conversation), `v` on the list opens the
//! conversation under its cursor beside, the thread panel splits off as a pane of its own,
//! `Ctrl+H/J/K/L` move the keyboard by place, a click focuses a pane, `z` zooms, `Space w =`
//! evens the room, `Ctrl+W` closes pane by pane, and a split with no room left is refused. At
//! 120x40 and 80x24 with two to four panes; screens are insta snapshots (Hangul masked).

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use slakio_core::i18n::Label;
use slakio_tui::app::{Focus, PaneHandle, PaneKind};

/// The panes on screen, in reading order, and where.
fn panes(d: &Demo) -> Vec<(PaneHandle, Rect)> {
    d.app.panes_on_screen().into_iter().map(|(p, r)| (p.handle(), r)).collect()
}

/// The pane with the keyboard.
fn focused(d: &Demo) -> PaneHandle {
    d.app.focused_pane().expect("a pane has the keyboard").handle()
}

/// The name of what pane `h` shows.
fn name(d: &Demo, h: PaneHandle) -> String {
    let p = d.app.pane_for(h).expect("open");
    let c = d.app.model.target(p.target()).expect("a conversation");
    let kind = if p.kind() == PaneKind::Thread { "thread of " } else { "" };
    format!("{kind}{}", c.name.line().as_str())
}

/// The area of pane `h` on screen.
fn rect(d: &Demo, h: PaneHandle) -> Rect {
    panes(d).into_iter().find(|(p, _)| *p == h).map(|(_, r)| r).expect("on screen")
}

fn label(d: &Demo, l: Label) -> String {
    d.app.i18n.label(l).to_string()
}

/// `backend` open, split beside (`v`) and the right one under (`s`): three panes.
fn three(w: u16, h: u16) -> Demo {
    let mut d = Demo::new(w, h);
    d.open("backend");
    d.keys("v");
    d.keys("s");
    d
}

#[test]
fn v_opens_beside_and_s_under_the_pane_each_keeping_its_own_selection() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    let first = focused(&d);
    let whole = rect(&d, first);
    d.keys("v");
    let second = focused(&d);
    assert_ne!(first, second, "the new pane has the keyboard");
    assert_eq!(name(&d, second), "backend", "nothing selected: the same conversation again");
    let (a, b) = (rect(&d, first), rect(&d, second));
    assert!(a.y == b.y && a.height == b.height && a.right() == b.x, "side by side: {a:?} {b:?}");
    assert_eq!(a.width + b.width, whole.width);
    assert!(a.width.abs_diff(b.width) <= 1, "even: {a:?} {b:?}");
    // Each pane has its own selection.
    d.keys("k k");
    let n = d.app.pane_for(second).unwrap().messages().len();
    assert_eq!(d.app.pane_for(second).unwrap().selected(), Some(n - 2));
    assert_eq!(d.app.pane_for(first).unwrap().selected(), None);
    d.keys("Escape");
    d.keys("s");
    let third = focused(&d);
    let (b, c) = (rect(&d, second), rect(&d, third));
    assert!(b.x == c.x && b.width == c.width && b.bottom() == c.y, "under: {b:?} {c:?}");
    assert_eq!(panes(&d).len(), 3);
    assert_eq!(d.app.open_panes().len(), 3);
    // `:vsplit` and `:split` do the same.
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.command("vsplit");
    assert_eq!(panes(&d).len(), 2);
    d.command("split");
    assert_eq!(panes(&d).len(), 3);
}

#[test]
fn a_split_opens_the_selected_thread_and_the_thread_panel_splits_off() {
    let mut d = Demo::new(120, 40);
    d.open("long-threads");
    let main = focused(&d);
    d.keys("g g");
    d.keys("v");
    let thread = focused(&d);
    assert_eq!(name(&d, thread), "thread of long-threads");
    assert!(rect(&d, main).right() == rect(&d, thread).x, "beside");
    // Again on the same message: the thread is open already, so it is focused.
    d.keys("ctrl+h");
    assert_eq!(focused(&d), main);
    d.keys("v");
    assert_eq!((focused(&d), d.app.open_panes().len()), (thread, 2), "never opened twice");
    // The thread panel (Enter) splits off under its pane with `s`: a pane of its own.
    d.keys("ctrl+h");
    d.keys("j");
    d.keys("enter");
    let panel = focused(&d);
    assert_eq!(name(&d, panel), "thread of long-threads");
    assert_eq!(d.app.open_panes().len(), 3);
    d.keys("s");
    assert_eq!(focused(&d), panel, "the panel itself, with its messages and selection");
    assert_eq!(d.app.open_panes().len(), 3);
    let (m, p) = (rect(&d, main), rect(&d, panel));
    assert!(m.x == p.x && m.bottom() == p.y, "under its pane: {m:?} {p:?}");
    // It is no panel any more: Enter on another message opens a new panel.
    d.keys("ctrl+k");
    assert_eq!(focused(&d), main);
    d.keys("j");
    d.keys("enter");
    assert_eq!(d.app.open_panes().len(), 4, "a new thread panel");
    assert!(d.app.pane_for(panel).is_some(), "the pane split off stays");
    // `v` in the panel splits it off beside its pane.
    let panel2 = focused(&d);
    d.keys("v");
    assert_eq!((focused(&d), d.app.open_panes().len()), (panel2, 4));
}

#[test]
fn the_list_v_opens_the_conversation_under_its_cursor_beside() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    let backend = focused(&d);
    d.list_cursor_on("incidents");
    d.keys("v");
    let incidents = focused(&d);
    assert_eq!(name(&d, incidents), "incidents");
    assert_eq!(panes(&d).len(), 2);
    assert!(rect(&d, backend).right() == rect(&d, incidents).x);
    // What is open already is focused instead.
    d.list_cursor_on("backend");
    d.keys("v");
    assert_eq!((focused(&d), panes(&d).len()), (backend, 2));
    // `Space w s` from the list opens under (`s` sorts To-dos there).
    d.list_cursor_on("deploys");
    d.keys("space w s");
    let deploys = focused(&d);
    assert_eq!(name(&d, deploys), "deploys");
    assert!(rect(&d, backend).bottom() == rect(&d, deploys).y, "under the pane the list opens into");
    // With nothing open, `v` opens as Enter does.
    let mut d = Demo::new(120, 40);
    d.list_cursor_on("general");
    d.keys("v");
    assert_eq!((name(&d, focused(&d)), panes(&d).len()), ("general".to_string(), 1));
}

#[test]
fn ctrl_h_j_k_l_move_by_place_and_tab_goes_round() {
    let mut d = three(120, 40);
    let ids: Vec<PaneHandle> = panes(&d).into_iter().map(|(p, _)| p).collect();
    let [left, top, bottom] = ids[..] else { panic!("three panes") };
    assert_eq!(focused(&d), bottom);
    d.keys("ctrl+k");
    assert_eq!(focused(&d), top);
    d.keys("ctrl+h");
    assert_eq!(focused(&d), left);
    d.keys("ctrl+l");
    assert_eq!(focused(&d), top, "of the two on the right, the top one");
    d.keys("ctrl+j");
    assert_eq!(focused(&d), bottom);
    d.keys("ctrl+j");
    assert_eq!(focused(&d), bottom, "nothing below");
    assert!(d.status_line().contains(&label(&d, Label::StatusNoPaneBelow)), "{}", d.status_line());
    d.keys("space w h");
    assert_eq!(focused(&d), left);
    d.keys("ctrl+h");
    assert_eq!(d.app.focus(), Focus::List, "from the leftmost pane, the list");
    // Tab goes round the panes in reading order.
    d.keys("tab");
    assert_eq!(focused(&d), left);
    d.keys("tab");
    assert_eq!(focused(&d), top);
    d.keys("tab");
    assert_eq!(focused(&d), bottom);
    d.keys("shift+tab");
    assert_eq!(focused(&d), top);
    // `h` and `l` go by place too.
    d.keys("h");
    assert_eq!(focused(&d), left);
    d.keys("l");
    assert_eq!(focused(&d), top);
}

#[test]
fn a_click_focuses_the_pane_under_it() {
    let mut d = three(120, 40);
    let [left, top, bottom] = panes(&d).into_iter().map(|(p, _)| p).collect::<Vec<_>>()[..] else { panic!() };
    for h in [left, top, bottom, left] {
        let r = rect(&d, h);
        d.mouse(MouseEventKind::Down(MouseButton::Left), r.x + r.width / 2, r.y + 2);
        d.mouse(MouseEventKind::Up(MouseButton::Left), r.x + r.width / 2, r.y + 2);
        d.pump();
        assert_eq!(focused(&d), h);
        // The next click is not a double click.
        d.now += std::time::Duration::from_secs(1);
    }
}

#[test]
fn z_zooms_the_pane_to_the_whole_tab_and_back() {
    let mut d = three(120, 40);
    let bottom = focused(&d);
    let work = d.app.areas().work;
    d.keys("z");
    assert_eq!(panes(&d), vec![(bottom, work)], "the pane alone, the whole work area");
    let s = d.screen();
    assert!(s.contains(&format!("[{}]", label(&d, Label::PaneZoomed))), "{s}");
    assert!(d.status_line().contains("Zoomed: z shows every pane again"), "{}", d.status_line());
    assert_eq!(d.app.open_panes().len(), 3, "the others stay open");
    d.keys("z");
    assert_eq!(panes(&d).len(), 3, "the layout as it was");
    // `:zoom`, then a move to another pane shows every pane again.
    d.command("zoom");
    assert_eq!(panes(&d).len(), 1);
    d.keys("ctrl+h");
    assert_eq!(panes(&d).len(), 3);
    assert_ne!(focused(&d), bottom);
    // Closing a pane of a zoomed tab shows the rest.
    d.keys("z");
    d.keys("ctrl+w");
    assert_eq!(panes(&d).len(), 2);
    // One pane: nothing to zoom.
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.keys("z");
    assert!(d.status_line().contains(&label(&d, Label::StatusZoomOne)), "{}", d.status_line());
    assert!(!d.screen().contains("[zoomed]"));
}

#[test]
fn space_w_equals_gives_the_panes_even_room() {
    let mut d = Demo::new(200, 50);
    d.open("backend");
    d.keys("v");
    d.keys("v");
    let widths = |d: &Demo| panes(d).into_iter().map(|(_, r)| r.width).collect::<Vec<_>>();
    let before = widths(&d);
    assert_eq!(before.len(), 3);
    assert!(before[0] > before[1] + 10, "halved, then halved again: {before:?}");
    d.keys("space w =");
    let after = widths(&d);
    assert_eq!(after.iter().sum::<u16>(), before.iter().sum::<u16>());
    assert!(after.iter().max().unwrap() - after.iter().min().unwrap() <= 1, "{after:?}");
    d.command("equalize");
    assert_eq!(widths(&d), after);
}

#[test]
fn ctrl_w_closes_pane_by_pane_down_to_an_empty_work_area() {
    let mut d = three(120, 40);
    let [left, top, bottom] = panes(&d).into_iter().map(|(p, _)| p).collect::<Vec<_>>()[..] else { panic!() };
    assert_eq!(focused(&d), bottom);
    d.keys("ctrl+w");
    assert_eq!(focused(&d), top, "the pane before it");
    assert_eq!(rect(&d, top).height, d.app.areas().work.height, "the one left takes the room");
    d.keys("ctrl+w");
    assert_eq!(focused(&d), left);
    assert_eq!(rect(&d, left), d.app.areas().work);
    d.keys("ctrl+w");
    assert!(d.app.open_panes().is_empty() && d.app.current_tab().is_none());
}

#[test]
fn a_split_with_no_room_left_is_refused_with_a_notice() {
    let mut d = Demo::new(80, 24);
    d.open("backend");
    d.keys("v");
    assert_eq!(panes(&d).len(), 2);
    d.keys("v");
    assert_eq!(panes(&d).len(), 2, "two panes of 25 columns: no room for a third beside");
    assert!(d.status_line().contains(&label(&d, Label::StatusSplitRoom)), "{}", d.status_line());
    d.keys("s");
    assert_eq!(panes(&d).len(), 3);
    d.keys("s");
    assert_eq!(panes(&d).len(), 3, "eleven rows: no room for a fourth under");
    for (_, r) in panes(&d) {
        assert!(r.width >= 24 && r.height >= 8, "{r:?}");
    }
}

#[test]
fn split_snapshots_with_two_to_four_panes() {
    for (w, h) in [(120, 40), (80, 24)] {
        let mut d = Demo::new(w, h);
        d.open("backend");
        d.list_cursor_on("incidents");
        d.keys("v");
        assert_eq!(panes(&d).len(), 2);
        insta::assert_snapshot!(format!("split_two_{w}x{h}"), d.snap());
        d.keys("g g");
        d.keys("s");
        assert_eq!(panes(&d).len(), 3);
        insta::assert_snapshot!(format!("split_three_{w}x{h}"), d.snap());
        d.keys("ctrl+h");
        d.list_cursor_on("deploys");
        d.keys("space w s");
        assert_eq!(panes(&d).len(), 4, "{}", d.screen());
        insta::assert_snapshot!(format!("split_four_{w}x{h}"), d.snap());
        d.keys("z");
        insta::assert_snapshot!(format!("split_zoomed_{w}x{h}"), d.snap());
    }
}

#[test]
fn which_key_and_help_list_the_split_keys() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.keys("?");
    let help = d.screen();
    for l in [Label::ActionPaneSplitRight, Label::ActionPaneSplitBelow, Label::ActionPaneZoom] {
        assert!(help.contains(&label(&d, l)), "{l:?}: {help}");
    }
}

/// The panes on screen that show a thread.
fn threads_on_screen(d: &Demo) -> usize {
    panes(d).into_iter().filter(|(p, _)| d.app.pane_for(*p).unwrap().kind() == PaneKind::Thread).count()
}

#[test]
fn v_beside_a_pane_with_its_thread_panel_splits_the_panel_off_so_all_three_show() {
    let mut d = Demo::new(120, 40);
    d.open("backend");
    let main = focused(&d);
    d.keys("k");
    d.keys("enter");
    let panel = focused(&d);
    assert_eq!(panes(&d).len(), 2);
    // Another message, then `v`: its thread beside, and the panel stays on screen.
    d.keys("h");
    assert_eq!(focused(&d), main);
    d.keys("k");
    d.keys("v");
    let new = focused(&d);
    assert_eq!(name(&d, new), "thread of backend");
    let shown: Vec<PaneHandle> = panes(&d).into_iter().map(|(p, _)| p).collect();
    assert_eq!(shown, vec![main, panel, new], "the three side by side:\n{}", d.screen());
    let widths: Vec<u16> = panes(&d).into_iter().map(|(_, r)| r.width).collect();
    assert!(widths.iter().max().unwrap() - widths.iter().min().unwrap() <= 1, "even: {widths:?}");
    assert_eq!(threads_on_screen(&d), 2);
    // By place, each is reached.
    d.keys("ctrl+h");
    assert_eq!(focused(&d), panel);
    d.keys("ctrl+h");
    assert_eq!(focused(&d), main);
    d.keys("l");
    assert_eq!(focused(&d), panel);
    // Where the three do not fit, the split is refused and the panel stays beside its pane.
    let mut d = Demo::new(80, 24);
    d.open("backend");
    d.keys("k");
    d.keys("enter");
    d.keys("h");
    d.keys("k");
    d.keys("v");
    assert!(d.status_line().contains(&label(&d, Label::StatusSplitRoom)), "{}", d.status_line());
    assert_eq!(d.app.open_panes().len(), 2);
}

#[test]
fn l_and_h_reach_a_panes_own_thread_panel_left_out_for_room_before_the_pane_beside() {
    for (w, h) in [(120, 40), (110, 30), (80, 24)] {
        let mut d = Demo::new(w, h);
        d.open("backend");
        d.keys("v");
        let right = focused(&d);
        d.keys("ctrl+h");
        let main = focused(&d);
        d.keys("k");
        d.keys("enter");
        let panel = focused(&d);
        assert_ne!(panel, main);
        d.keys("h");
        assert_eq!(focused(&d), main, "{w}x{h}");
        assert!(panes(&d).iter().all(|(p, _)| *p != panel), "left out for room at {w}x{h}");
        d.keys("l");
        assert_eq!(focused(&d), panel, "its own panel first, not the pane beside, at {w}x{h}");
        d.keys("l");
        assert_eq!(focused(&d), right, "{w}x{h}");
        // The pane beside and the one the panel shows: never the same thread twice.
        assert!(threads_on_screen(&d) <= 1, "{}", d.screen());
    }
}

#[test]
fn the_list_comes_back_while_the_thread_panel_is_left_out_for_room() {
    for (w, h) in [(110, 30), (80, 24)] {
        let mut d = Demo::new(w, h);
        d.open("backend");
        d.keys("v");
        d.keys("ctrl+h");
        d.keys("k");
        d.keys("enter");
        assert!(d.app.areas().list.is_none(), "the panel shown takes the list's room at {w}x{h}");
        d.keys("h");
        assert!(threads_on_screen(&d) == 0, "the panel is left out: {}", d.screen());
        assert!(d.app.areas().list.is_some(), "no panel shown, the list stays at {w}x{h}:\n{}", d.screen());
    }
}

#[test]
fn a_split_that_is_done_drops_the_no_room_notice() {
    let mut d = Demo::new(80, 24);
    d.open("backend");
    d.keys("v");
    d.keys("v");
    let room = label(&d, Label::StatusSplitRoom);
    assert!(d.status_line().contains(&room), "{}", d.status_line());
    d.keys("s");
    assert_eq!(panes(&d).len(), 3);
    assert!(!d.status_line().contains(&room), "{}", d.status_line());
    // A move that is done drops `No pane below` too.
    d.keys("ctrl+j");
    let below = label(&d, Label::StatusNoPaneBelow);
    assert!(d.status_line().contains(&below), "{}", d.status_line());
    d.keys("ctrl+k");
    assert!(!d.status_line().contains(&below), "{}", d.status_line());
}

#[test]
fn a_move_by_place_and_back_returns_to_the_pane_it_came_from() {
    // Left: one pane over another; right: one the whole height.
    let mut d = Demo::new(120, 40);
    d.open("backend");
    d.keys("v");
    let tall = focused(&d);
    d.keys("ctrl+h");
    let top = focused(&d);
    d.keys("s");
    let bottom = focused(&d);
    assert!(rect(&d, top).bottom() == rect(&d, bottom).y && rect(&d, top).right() == rect(&d, tall).x);
    d.keys("ctrl+l");
    assert_eq!(focused(&d), tall);
    d.keys("ctrl+h");
    assert_eq!(focused(&d), bottom, "back where it came from");
    d.keys("ctrl+k");
    d.keys("l");
    d.keys("h");
    assert_eq!(focused(&d), top);
}

#[test]
fn narrow_pane_titles_and_empty_threads_are_cut_whole_or_with_an_ellipsis() {
    let mut d = Demo::new(80, 24);
    d.open("long-threads");
    d.keys("g g");
    d.keys("v");
    assert_eq!(name(&d, focused(&d)), "thread of long-threads");
    let screen = d.screen();
    let r = rect(&d, focused(&d));
    let top: String = screen.lines().nth(usize::from(r.y)).unwrap().chars().skip(usize::from(r.x)).collect();
    let top: String = top.chars().take(usize::from(r.width)).collect();
    assert!(top.contains('…') && top.ends_with("──╮"), "{top:?}");
    // A thread with no replies, in a narrow pane: its words whole, the key left out.
    let mut d = Demo::new(80, 24);
    d.open("backend");
    d.keys("k");
    d.keys("v");
    assert_eq!(name(&d, focused(&d)), "thread of backend");
    let none = label(&d, Label::ThreadNoRepliesReadOnly);
    let screen = d.screen();
    assert!(screen.contains(&none) && !screen.contains(&format!("{none} ·")), "{screen}");
}

#[test]
fn help_says_what_v_does_on_the_list_and_on_a_pane_and_pane_moves_are_found_by_pane() {
    for lang in [slakio_core::i18n::Lang::En, slakio_core::i18n::Lang::Ko] {
        let mut d = Demo::with(80, 200, lang, Default::default());
        d.open("backend");
        // On a pane, `v` splits; on the list, it opens what is under the cursor beside.
        d.keys("?");
        let screen = d.screen();
        let moves = [Label::ActionPaneLeft, Label::ActionPaneRight, Label::ActionFocusUp, Label::ActionFocusDown];
        for l in [Label::ActionPaneSplitRight, Label::ActionPaneSplitBelow].into_iter().chain(moves) {
            assert!(screen.contains(&label(&d, l)), "{lang:?} {l:?} whole at 80:\n{screen}");
        }
        d.keys("escape");
        d.list_cursor_on("incidents");
        d.keys("?");
        let beside = label(&d, Label::ActionPaneOpenBeside);
        d.keys("/");
        d.type_text(beside.split_whitespace().next().unwrap());
        let screen = d.screen();
        let row = screen.lines().find(|l| l.contains(&beside)).unwrap_or_else(|| panic!("{lang:?}:\n{screen}"));
        assert!(row.trim_end_matches(['│', ' ']).ends_with('v'), "{lang:?}: {row}");
        d.keys("escape");
        d.keys("ctrl+l");
        // One word for a pane: searching it finds the moves, zoom and close.
        let word = label(&d, Label::ActionPaneClose).split_whitespace().find(|w| w.len() > 1).unwrap().to_lowercase();
        let word = if lang == slakio_core::i18n::Lang::En { "pane".to_string() } else { word };
        d.keys("escape");
        d.keys("?");
        d.keys("/");
        d.type_text(&word);
        let found = d.screen();
        for l in moves.into_iter().chain([Label::ActionPaneZoom, Label::ActionPaneClose]) {
            assert!(label(&d, l).to_lowercase().contains(&word), "{lang:?} {l:?} says {word}");
            assert!(found.contains(&label(&d, l)), "{lang:?} /{word} finds {l:?}:\n{found}");
        }
    }
}
