//! Navigation in the list panel: the workspace chip on its title, the view switcher under it, a
//! row per view (or one folded row, `Space v`). By keyboard: `Ctrl+R` or `Space r` from anywhere
//! outside text, or `Tab`/`Shift+Tab` round the panels, then `j`/`k`/arrows (on into the list
//! and back) and `Enter`, `Esc` to leave; `[` / `]` in the list; `Space W` opens the workspace
//! switcher anywhere. By mouse: a click. The counts read as the list's pills, every one of them
//! whole at every width the list panel takes, with icons on or off, folded or not. No row runs
//! across the whole screen but the status line.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use slakio_core::i18n::Lang;
use slakio_tui::app::shell::View;
use slakio_tui::app::{Effect, Focus, Overlay, PaneKind, Settings};
use slakio_tui::navbar::{GLYPH_SLOT, Part};
use std::time::Duration;

/// The screen's row `y`.
fn line(d: &Demo, y: usize) -> String {
    d.screen().lines().nth(y).unwrap().to_string()
}

/// The list panel's part of row `y` (inside its borders).
fn in_list(d: &Demo, y: usize) -> String {
    let w = usize::from(d.app.areas().list.unwrap().width);
    line(d, y).chars().take(w).collect()
}

fn click(d: &mut Demo, x: u16, y: u16) {
    d.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    d.mouse(MouseEventKind::Up(MouseButton::Left), x, y);
    d.pump();
}

/// The screen row of view `v` on the view switcher.
fn view_y(d: &Demo, v: View) -> u16 {
    let item = View::ALL.iter().position(|x| *x == v).unwrap();
    d.app.view_rows()[item].area.y
}

#[test]
fn no_row_runs_across_the_whole_screen_but_the_status_line() {
    for (w, h) in [(120, 40), (100, 30), (80, 24)] {
        let d = Demo::new(w, h);
        let screen = d.screen();
        let rows: Vec<&str> = screen.lines().collect();
        let list = d.app.areas().list.unwrap();
        assert_eq!((list.x, list.y), (0, 0), "{w}x{h}: the list panel starts at the top");
        assert!(rows[0].starts_with("╭ ▌A company ▾"), "{w}x{h}: the chip is the list's title: {}", rows[0]);
        for (y, row) in rows[..rows.len() - 1].iter().enumerate() {
            let cells: Vec<char> = row.chars().collect();
            let split = cells.get(usize::from(list.width) - 1..usize::from(list.width) + 1);
            assert!(
                matches!(split, Some(['╮' | '│' | '┤' | '╯', '╭' | '│' | '╰' | ' ' | '├'])),
                "{w}x{h} row {y}: the list panel's border splits it: {row}"
            );
        }
    }
}

#[test]
fn ctrl_r_and_space_r_reach_the_view_switcher_from_the_list_and_a_pane() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+r");
    assert_eq!(d.app.focus(), Focus::ViewSwitcher, "from the list");
    d.keys("ctrl+r");
    assert_eq!(d.app.focus(), Focus::List, "the same key leaves it");
    d.open("backend");
    assert_eq!(d.focused_kind(), Some(PaneKind::Conversation));
    d.keys("space r");
    assert_eq!(d.app.focus(), Focus::ViewSwitcher, "from a pane, by the leader key");
    d.keys("esc");
    assert_eq!(d.app.focus(), Focus::List);
    d.command("nav");
    assert_eq!(d.app.focus(), Focus::ViewSwitcher, ":nav, and the old :rail too");
    d.keys("esc");
    d.command("rail");
    assert_eq!(d.app.focus(), Focus::ViewSwitcher);
    // A hidden list panel shows again with its view switcher.
    d.keys("esc space e");
    assert!(d.app.areas().list.is_none());
    d.keys("ctrl+r");
    assert_eq!(d.app.focus(), Focus::ViewSwitcher);
    assert!(d.app.areas().list.is_some());
}

#[test]
fn tab_goes_round_the_view_switcher_the_list_and_the_panes() {
    let mut d = Demo::new(120, 40);
    d.keys("shift+tab");
    assert_eq!(d.app.focus(), Focus::ViewSwitcher, "the view switcher comes before the list");
    d.keys("tab");
    assert_eq!(d.app.focus(), Focus::List);
    d.open("long-threads");
    d.keys("g g enter");
    assert_eq!(d.focused_kind(), Some(PaneKind::Thread));
    d.keys("tab");
    assert_eq!(d.app.focus(), Focus::ViewSwitcher, "after the thread panel comes the view switcher");
    d.keys("tab tab tab");
    assert_eq!(d.focused_kind(), Some(PaneKind::Thread), "and round again");
    d.keys("f6");
    assert_eq!(d.app.focus(), Focus::ViewSwitcher, "F6 is Tab");
}

#[test]
fn down_the_switcher_j_k_and_enter_show_a_view_and_run_on_into_the_list() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+r");
    assert_eq!(d.app.nav_cursor(), 0, "the cursor starts on the view shown (Home)");
    d.keys("j enter");
    assert_eq!((d.app.view(), d.app.focus()), (View::Dms, Focus::List), "DMs, then the list");
    d.keys("ctrl+r down down up enter");
    assert_eq!(d.app.view(), View::Activity);
    d.keys("ctrl+r home enter");
    assert_eq!(d.app.view(), View::Home);
    // Down from the last view is the list's first row; up from there, the last view again.
    d.keys("ctrl+r G j");
    assert_eq!((d.app.focus(), d.app.list_cursor()), (Focus::List, 0), "on into the list, its first row");
    d.keys("k");
    assert_eq!((d.app.focus(), d.app.nav_cursor()), (Focus::ViewSwitcher, 4), "back up onto Later");
    d.keys("k k");
    assert_eq!(d.app.nav_cursor(), 2);
    d.keys("esc");
    assert_eq!((d.app.focus(), d.app.view()), (Focus::List, View::Home), "Esc shows nothing new");
    d.keys("]");
    assert_eq!((d.app.view(), d.app.focus()), (View::Dms, Focus::List), "] the next view");
    d.keys("[ [");
    assert_eq!(d.app.view(), View::Later, "[ the one before, round again");
    d.keys("]");
    d.keys("space W");
    assert_eq!(d.app.switcher(), Some(0), "Space W, on the workspace shown");
    d.keys("j enter");
    assert_eq!((d.app.workspace(), d.app.view(), d.app.focus()), (1, View::Home, Focus::List));
    assert!(line(&d, 0).contains("B side"), "{}", line(&d, 0));
    d.keys("space W k esc");
    assert_eq!((d.app.overlay(), d.app.workspace()), (None, 1), "Esc switches nothing");
}

#[test]
fn space_v_folds_the_switcher_to_the_view_shown_and_saves_it() {
    let mut d = Demo::new(120, 40);
    d.keys("space a");
    d.keys("space v");
    assert_eq!(d.app.view_rows().len(), 1, "one row");
    assert!(in_list(&d, 1).contains("▸ Activity"), "{}", in_list(&d, 1));
    assert!(in_list(&d, 2).starts_with("├─"), "the rule right under it: {}", in_list(&d, 2));
    let saved = d.app.take_effects();
    assert!(
        saved.iter().any(|e| matches!(e, Effect::Save { key: "nav_rows", value } if value == "collapsed")),
        "{saved:?}"
    );
    // Folded, j from the row is the list and k comes back to it; Enter unfolds.
    d.keys("ctrl+r");
    assert_eq!(View::ALL[d.app.nav_cursor()], View::Activity);
    d.keys("j");
    assert_eq!(d.app.focus(), Focus::List);
    d.keys("ctrl+r enter");
    assert_eq!((d.app.view_rows().len(), d.app.focus()), (5, Focus::ViewSwitcher), "unfolded, still on the views");
    // A click on the folded row unfolds it too.
    d.keys("space v");
    click(&mut d, 10, 1);
    assert_eq!(d.app.view_rows().len(), 5);
    d.command("navrows");
    assert_eq!(d.app.view_rows().len(), 1, ":navrows");
}

#[test]
fn a_click_shows_a_view_or_the_chip_opens_the_switcher_whose_rows_switch() {
    let mut d = Demo::with(120, 40, Lang::En, Settings { icons: true, ..Settings::default() });
    let y = view_y(&d, View::Dms);
    click(&mut d, 20, y);
    assert_eq!((d.app.view(), d.app.focus()), (View::Dms, Focus::List), "anywhere on the row");
    // Either cell of a glyph is its view's.
    let rows = d.app.view_rows();
    let g = rows[2].pieces.iter().find(|p| p.part == Part::Glyph).unwrap();
    assert_eq!(g.width, GLYPH_SLOT);
    click(&mut d, g.x + 1, rows[2].area.y);
    assert_eq!(d.app.view(), View::Activity);
    click(&mut d, 4, 0);
    assert_eq!(d.app.overlay(), Some(Overlay::Switcher));
    let b = d.app.switcher_box().unwrap();
    assert_eq!(b.y, 1, "under the chip");
    click(&mut d, b.x + 4, b.y + 2);
    assert_eq!((d.app.workspace(), d.app.overlay()), (1, None), "the second row: workspace B");
    click(&mut d, 4, 0);
    click(&mut d, 100, 30);
    assert_eq!((d.app.overlay(), d.app.workspace()), (None, 1), "a click outside closes it");
    // The rule under the switcher opens nothing.
    let rule = d.app.list_parts().unwrap().rule.unwrap();
    click(&mut d, 4, rule);
    assert_eq!((d.app.overlay(), d.app.view()), (None, View::Home));
}

#[test]
fn the_counts_say_what_the_list_says() {
    let d = Demo::new(200, 40);
    let convs = d.app.model.conversations();
    let a_dms: u32 = convs
        .iter()
        .filter(|c| c.workspace.as_str() == "TDEMOA" && c.is_dm() && c.unread > 0 && !c.muted)
        .map(|c| c.unread)
        .sum();
    let mentions: u32 = convs.iter().map(|c| c.mentions).sum();
    let b_mentions: u32 = convs.iter().filter(|c| c.workspace.as_str() == "TDEMOB").map(|c| c.mentions).sum();
    let chip = in_list(&d, 0);
    let dms = in_list(&d, 2);
    let activity = in_list(&d, 3);
    assert!(dms.contains("DMs") && dms.trim_end().ends_with(&format!("●{a_dms} │")), "unread DM messages: {dms}");
    assert!(activity.trim_end().ends_with(&format!("@{mentions} │")), "{activity}");
    assert!(chip.contains(" ▌A company ▾ · B "), "no count beside the name shown; B's after the caret: {chip}");
    if b_mentions > 0 {
        assert!(chip.contains(&format!("· B @{b_mentions}")), "{chip}");
    }
    let files = in_list(&d, 4);
    assert!(files.contains("Files") && !files.contains(['@', '●']), "none where nothing is unread: {files}");
    // The status line repeats no count: one number for one thing on the screen.
    let status = d.status_line();
    assert!(!status.contains(&format!("@{mentions}")) && !status.contains(&format!(" {a_dms}")), "{status}");
}

#[test]
fn every_count_is_whole_at_every_width_the_list_panel_takes() {
    let mut widths = std::collections::BTreeSet::new();
    for (w, h) in (130..=170).map(|w| (w, 40)).chain([(120, 40), (100, 30), (80, 24), (50, 20)]) {
        for icons in [false, true] {
            for folded in [false, true] {
                let mut d = Demo::with(w, h, Lang::En, Settings { icons, ..Settings::default() });
                d.app.fold_views(folded);
                let list = d.app.areas().list.unwrap();
                widths.insert(list.width);
                assert!(in_list(&d, 0).contains("A company"), "{w} {icons}: {}", in_list(&d, 0));
                let rows = d.app.view_rows();
                let shown: Vec<usize> = if folded { vec![0] } else { (0..View::ALL.len()).collect() };
                assert_eq!(rows.len(), shown.len(), "{w} {icons} {folded}");
                for (row, &i) in rows.iter().zip(&shown) {
                    let want = d.app.view_unread(0, View::ALL[i]).badge();
                    let got = row.pieces.iter().find(|p| p.part == Part::Badge).map(|p| p.text.clone());
                    assert_eq!(got, want, "{w} (panel {}) icons {icons} folded {folded}: view {i}", list.width);
                    let text = in_list(&d, usize::from(row.area.y));
                    if let Some(b) = want {
                        assert!(text.contains(&b), "drawn whole: {text}");
                    }
                }
                let rule = usize::from(d.app.list_parts().unwrap().rule.unwrap());
                assert!(in_list(&d, rule).starts_with("├─"), "a rule under them: {}", in_list(&d, rule));
            }
        }
    }
    assert_eq!(widths.into_iter().collect::<Vec<_>>(), (30..=36).collect::<Vec<_>>(), "every panel width");
}

#[test]
fn a_glyph_is_written_with_its_blank_so_a_wide_glyph_never_shifts_the_text_after_it() {
    let d = Demo::with(120, 40, Lang::En, Settings { icons: true, ..Settings::default() });
    let bar = d.app.view_rows().remove(0);
    let g = bar.pieces.iter().find(|p| p.part == Part::Glyph).unwrap();
    let buf = d.buffer();
    assert_eq!(buf[(g.x, 1)].symbol(), format!("{} ", g.text), "one cell holds the glyph and its blank");
    // The bytes for the terminal: after the glyph and its blank, the next text is placed by an
    // explicit cursor move, so a terminal that drew the glyph two cells wide puts it right.
    let bytes = String::from_utf8_lossy(&d.terminal_bytes()).to_string();
    let after = bytes.split(&format!("{} ", g.text)).nth(1).expect("the glyph is written");
    let label = bar.pieces.iter().find(|p| p.part == Part::Label).unwrap();
    let moved = format!("\u{1b}[2;{}H", g.x + GLYPH_SLOT + 1);
    assert!(
        after.starts_with(&moved) || after.starts_with('\u{1b}') && after.contains(&moved),
        "{:?}",
        &after[..40.min(after.len())]
    );
    assert_eq!(label.x, g.x + GLYPH_SLOT);
}

#[test]
fn the_hint_line_says_how_to_reach_the_views() {
    // In the list the views are right above: `[` / `]`.
    let d = Demo::new(160, 40);
    assert!(d.status_line().contains("[/] views"), "{}", d.status_line());
    let mut d = Demo::new(120, 40);
    d.open("backend");
    assert!(d.status_line().contains("Ctrl+R views"), "from a pane: {}", d.status_line());
    d.keys("ctrl+r");
    let s = d.status_line();
    assert!(s.contains("Enter show") && s.contains("j/k move") && s.contains("Esc back"), "{s}");
}

#[test]
fn which_key_and_the_help_list_the_view_switcher_key() {
    let mut d = Demo::new(120, 40);
    d.keys("space");
    d.now += Duration::from_millis(400);
    d.app.on_tick(d.now);
    let s = d.screen();
    assert!(s.lines().any(|l| l.contains("│ r ") && l.contains("View switcher")), "{s}");
    d.keys("esc ?");
    d.type_text("/view switcher");
    let s = d.screen();
    assert!(s.contains("Ctrl+R / Space r"), "{s}");
}

#[test]
fn a_narrow_screen_keeps_the_workspace_and_the_view_in_the_breadcrumb() {
    let mut d = Demo::new(80, 24);
    d.open("long-threads");
    d.keys("g g enter");
    assert!(d.app.areas().list.is_none(), "the thread panel took the list's room");
    let s = d.status_line();
    assert!(s.contains("Home › "), "the view, then the place: {s}");
    assert!(d.app.view_rows().is_empty() && d.app.chip_bar().is_none());
    // The views stay a key away, and the list comes back with them.
    d.keys("space d");
    assert_eq!((d.app.view(), d.app.focus()), (View::Dms, Focus::List));
    assert!(d.app.areas().list.is_some());
}

#[test]
fn a_frame_that_changes_only_the_glyph_rewrites_its_slot_and_moves_before_the_next_text() {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;
    let area = Rect::new(0, 0, 12, 1);
    let mut before = Buffer::empty(area);
    before.set_string(0, 0, " ", Style::new());
    before[(1, 0)].set_symbol("\u{F02DC} ");
    before[(2, 0)].set_symbol(" ");
    before.set_string(3, 0, " Home ●2", Style::new());
    let mut after = before.clone();
    after[(1, 0)].set_symbol("\u{F0361} ");
    let diff = before.diff(&after);
    let xs: Vec<u16> = diff.iter().map(|(x, _, _)| *x).collect();
    assert_eq!(xs, [1], "only the glyph's cell: its blank goes with it, the text is untouched");
    // The count changes too: the text is written from its own column, not right after the glyph.
    after.set_string(9, 0, "●3", Style::new());
    let xs: Vec<u16> = before.diff(&after).iter().map(|(x, _, _)| *x).collect();
    assert_eq!(xs, [1, 10], "{xs:?}");
}
