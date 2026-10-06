//! The top bar: the workspace chip and the views across the screen's first row. By keyboard:
//! `Ctrl+R` or `Space r` from anywhere outside text, or `Tab`/`Shift+Tab` round the panels,
//! then `h`/`l`/arrows and `Enter` (the chip opens the workspace switcher), `Esc` to leave;
//! `Space W` opens the switcher anywhere. By mouse: a click. The counts read as the list's
//! pills; the bar never wraps, its glyphs keep two cells, and it reads at every width with
//! icons on or off.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use slakio_core::i18n::Lang;
use slakio_tui::app::shell::View;
use slakio_tui::app::{Focus, Overlay, PaneKind, Settings};
use slakio_tui::navbar::{GLYPH_SLOT, Part};
use std::time::Duration;

fn top(d: &Demo) -> String {
    d.screen().lines().next().unwrap().to_string()
}

fn click(d: &mut Demo, x: u16, y: u16) {
    d.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    d.mouse(MouseEventKind::Up(MouseButton::Left), x, y);
    d.pump();
}

/// The column of view `v`'s label (or glyph) on the bar.
fn view_x(d: &Demo, v: View) -> u16 {
    let item = 1 + View::ALL.iter().position(|x| *x == v).unwrap();
    d.app.nav_bar().unwrap().span(item).unwrap().0 + 1
}

#[test]
fn ctrl_r_and_space_r_reach_the_top_bar_from_the_list_and_a_pane() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+r");
    assert_eq!(d.app.focus(), Focus::Nav, "from the list");
    d.keys("ctrl+r");
    assert_eq!(d.app.focus(), Focus::List, "the same key leaves it");
    d.open("backend");
    assert_eq!(d.focused_kind(), Some(PaneKind::Conversation));
    d.keys("space r");
    assert_eq!(d.app.focus(), Focus::Nav, "from a pane, by the leader key");
    d.keys("esc");
    assert_eq!(d.app.focus(), Focus::List);
    d.command("nav");
    assert_eq!(d.app.focus(), Focus::Nav, ":nav, and the old :rail too");
    d.keys("j");
    d.command("rail");
    assert_eq!(d.app.focus(), Focus::Nav);
}

#[test]
fn tab_goes_round_the_top_bar_the_list_and_the_panes() {
    let mut d = Demo::new(120, 40);
    d.keys("shift+tab");
    assert_eq!(d.app.focus(), Focus::Nav, "the top bar comes before the list");
    d.keys("tab");
    assert_eq!(d.app.focus(), Focus::List);
    d.open("long-threads");
    d.keys("g g enter");
    assert_eq!(d.focused_kind(), Some(PaneKind::Thread));
    d.keys("tab");
    assert_eq!(d.app.focus(), Focus::Nav, "after the thread panel comes the top bar");
    d.keys("tab tab tab");
    assert_eq!(d.focused_kind(), Some(PaneKind::Thread), "and round again");
    d.keys("f6");
    assert_eq!(d.app.focus(), Focus::Nav, "F6 is Tab");
}

#[test]
fn along_the_bar_h_l_and_enter_show_a_view_and_the_chip_opens_the_switcher() {
    let mut d = Demo::new(120, 40);
    d.keys("ctrl+r");
    assert_eq!(d.app.nav_cursor(), 1, "the cursor starts on the view shown (Home)");
    d.keys("l enter");
    assert_eq!((d.app.view(), d.app.focus()), (View::Dms, Focus::List), "DMs, then the list");
    d.keys("ctrl+r right right left enter");
    assert_eq!(d.app.view(), View::Activity);
    d.keys("ctrl+r home enter");
    assert_eq!(d.app.overlay(), Some(Overlay::Switcher), "the workspace chip");
    assert_eq!(d.app.switcher(), Some(0));
    d.keys("j enter");
    assert_eq!((d.app.workspace(), d.app.view(), d.app.focus()), (1, View::Home, Focus::List));
    assert!(top(&d).contains("B side"), "{}", top(&d));
    d.keys("space W");
    assert_eq!(d.app.switcher(), Some(1), "Space W, on the workspace shown");
    d.keys("k esc");
    assert_eq!((d.app.overlay(), d.app.workspace()), (None, 1), "Esc switches nothing");
    // In the bar, j or down goes back to the list.
    d.keys("ctrl+r j");
    assert_eq!(d.app.focus(), Focus::List);
}

#[test]
fn a_click_shows_a_view_or_opens_the_switcher_whose_rows_switch() {
    let mut d = Demo::with(120, 40, Lang::En, Settings { icons: true, ..Settings::default() });
    let x = view_x(&d, View::Dms);
    click(&mut d, x, 0);
    assert_eq!((d.app.view(), d.app.focus()), (View::Dms, Focus::List));
    // Either cell of a glyph is its view's.
    let bar = d.app.nav_bar().unwrap();
    let g = bar.pieces.iter().find(|p| p.part == Part::Glyph && p.item == Some(3)).unwrap();
    assert_eq!(g.width, GLYPH_SLOT);
    click(&mut d, g.x + 1, 0);
    assert_eq!(d.app.view(), View::Activity);
    click(&mut d, 3, 0);
    assert_eq!(d.app.overlay(), Some(Overlay::Switcher));
    let b = d.app.switcher_box().unwrap();
    click(&mut d, b.x + 4, b.y + 2);
    assert_eq!((d.app.workspace(), d.app.overlay()), (1, None), "the second row: workspace B");
    click(&mut d, 3, 0);
    click(&mut d, 100, 30);
    assert_eq!((d.app.overlay(), d.app.workspace()), (None, 1), "a click outside closes it");
}

#[test]
fn the_counts_say_what_the_list_says() {
    let d = Demo::new(160, 40);
    let convs = d.app.model.conversations();
    let a_dms: u32 = convs
        .iter()
        .filter(|c| c.workspace.as_str() == "TDEMOA" && c.is_dm() && c.unread > 0 && !c.muted)
        .map(|c| c.unread)
        .sum();
    let mentions: u32 = convs.iter().map(|c| c.mentions).sum();
    let b_mentions: u32 = convs.iter().filter(|c| c.workspace.as_str() == "TDEMOB").map(|c| c.mentions).sum();
    let line = top(&d);
    assert!(line.contains(&format!("DMs ●{a_dms}")), "this workspace's unread DM messages, never red: {line}");
    assert!(line.contains(&format!("Activity @{mentions}")), "{line}");
    assert!(line.contains(" ▌A company ▾ · B "), "no count beside the name shown; B's after the caret: {line}");
    if b_mentions > 0 {
        assert!(line.contains(&format!("· B @{b_mentions}")), "{line}");
    }
    assert!(line.contains("Files  Later"), "no count where nothing is unread: {line}");
    // The status line repeats no count: one number for one thing on the screen.
    let status = d.status_line();
    assert!(!status.contains(&format!("@{mentions}")) && !status.contains(&format!(" {a_dms}")), "{status}");
}

#[test]
fn the_bar_reads_at_every_width_with_icons_on_or_off_and_never_wraps() {
    for icons in [false, true] {
        for (w, h) in [(120, 40), (100, 30), (80, 24)] {
            let d = Demo::with(w, h, Lang::En, Settings { icons, ..Settings::default() });
            let line = top(&d);
            assert!(line.contains("A company"), "{w} {icons}: {line}");
            for word in ["Home", "DMs", "Activity", "Files", "Later"] {
                assert!(line.contains(word), "full words at {w} (icons {icons}): {line}");
            }
            assert!(!d.screen().lines().nth(1).unwrap().contains("Later"), "one row only");
        }
        let d = Demo::with(50, 20, Lang::En, Settings { icons, ..Settings::default() });
        let bar = d.app.nav_bar().unwrap();
        assert!(bar.pieces.iter().map(|p| p.width).sum::<u16>() <= 50);
        assert!(bar.pieces.iter().any(|p| p.part == Part::Badge), "the counts stay at 50 wide");
    }
    let d = Demo::new(50, 20);
    assert!(top(&d).contains(" H ") && top(&d).contains(" L"), "letters only when narrow: {}", top(&d));
}

#[test]
fn a_glyph_is_written_with_its_blank_so_a_wide_glyph_never_shifts_the_text_after_it() {
    let d = Demo::with(120, 40, Lang::En, Settings { icons: true, ..Settings::default() });
    let bar = d.app.nav_bar().unwrap();
    let g = bar.pieces.iter().find(|p| p.part == Part::Glyph).unwrap();
    let buf = d.buffer();
    assert_eq!(buf[(g.x, 0)].symbol(), format!("{} ", g.text), "one cell holds the glyph and its blank");
    // The bytes for the terminal: after the glyph and its blank, the next text is placed by an
    // explicit cursor move, so a terminal that drew the glyph two cells wide puts it right.
    let bytes = String::from_utf8_lossy(&d.terminal_bytes()).to_string();
    let after = bytes.split(&format!("{} ", g.text)).nth(1).expect("the glyph is written");
    let label = bar.pieces.iter().find(|p| p.part == Part::Label && p.item == g.item).unwrap();
    let moved = format!("\u{1b}[1;{}H", g.x + GLYPH_SLOT + 1);
    assert!(
        after.starts_with(&moved) || after.starts_with('\u{1b}') && after.contains(&moved),
        "{:?}",
        &after[..40.min(after.len())]
    );
    assert_eq!(label.x, g.x + GLYPH_SLOT + 1);
}

#[test]
fn the_hint_line_says_how_to_reach_the_top_bar() {
    // On the list it comes after the list's own keys (the bar is in sight anyway).
    let d = Demo::new(160, 40);
    assert!(d.status_line().contains("Ctrl+R top bar"), "{}", d.status_line());
    let mut d = Demo::new(120, 40);
    d.open("backend");
    assert!(d.status_line().contains("Ctrl+R top bar"), "from a pane too: {}", d.status_line());
    d.keys("ctrl+r");
    let s = d.status_line();
    assert!(s.contains("Enter show") && s.contains("h/l move") && s.contains("Esc back"), "{s}");
}

#[test]
fn which_key_and_the_help_list_the_top_bar_key() {
    let mut d = Demo::new(120, 40);
    d.keys("space");
    d.now += Duration::from_millis(400);
    d.app.on_tick(d.now);
    let s = d.screen();
    assert!(s.lines().any(|l| l.starts_with("│ r ") && l.contains("Top bar")), "{s}");
    d.keys("esc ?");
    d.type_text("/top bar");
    let s = d.screen();
    assert!(s.contains("Ctrl+R / Space r"), "{s}");
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
