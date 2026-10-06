//! What the review of the reworked interface found, each pinned by a test that failed before
//! its fix: the help's cursor on its surface, orphaned periods in questions, the hint line at
//! 80 columns, the icons question's keys, the collapsed rail's marks, the which-key popup over a
//! dimmed screen, and details of dividers, the place in the status line and contrast.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::style::Color;
use slakio_core::i18n::Lang;
use slakio_tui::app::{PaneKind, Settings, WHICH_KEY_DELAY};
use slakio_tui::screen;
use slakio_tui::theme::{Background, resolve};

fn tokyo(w: u16, h: u16) -> Demo {
    let mut d = Demo::with(w, h, Lang::En, Settings::default());
    d.app.theme = resolve("tokyo-night", true, Background::Dark);
    d
}

fn luminance(c: Color) -> f64 {
    let Color::Rgb(r, g, b) = c else { return 0.0 };
    let lin = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

fn contrast(a: Color, b: Color) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[test]
fn the_help_cursor_is_a_bar_on_its_surface_in_a_truecolor_theme() {
    let mut d = tokyo(120, 40);
    d.keys("? j");
    let t = d.app.theme.clone();
    let buf = d.buffer();
    let rows = (0..40).filter(|&y| (10..100).filter(|&x| buf[(x, y)].bg == t.selection).count() > 50).count();
    assert_eq!(rows, 1, "one row of the help is the cursor bar");
}

#[test]
fn text_on_a_selection_bar_stays_readable() {
    let mut d = tokyo(120, 40);
    d.open("backend");
    d.keys("k");
    let t = d.app.theme.clone();
    let buf = d.buffer();
    for c in buf.content().iter().filter(|c| c.bg == t.selection && !c.symbol().trim().is_empty()) {
        assert!(contrast(c.fg, c.bg) >= 3.0, "{:?} on {:?}: {:.2}", c.fg, c.bg, contrast(c.fg, c.bg));
    }
}

#[test]
fn questions_never_leave_a_period_on_its_own_line() {
    // Inside the question's box (what shows of the screen under it, dimmed, is not its text).
    let orphan = |s: &str| {
        let lines: Vec<Vec<char>> = s.lines().map(|l| l.chars().collect()).collect();
        let Some((top, from)) = lines.iter().enumerate().find_map(|(y, l)| {
            let line: String = l.iter().collect();
            ["╭ Nerd Font icons?", "╭ Quit slakio?"]
                .iter()
                .find_map(|t| line.find(t))
                .map(|b| (y, line[..b].chars().count()))
        }) else {
            return false;
        };
        let to = (from + 1..lines[top].len()).find(|&x| lines[top][x] == '╮').unwrap_or(lines[top].len());
        lines[top..].iter().take_while(|l| l.get(from) != Some(&'╰')).any(|l| {
            let inside: String = l.iter().skip(from + 1).take(to - from - 1).collect();
            inside.split('│').any(|part| part.trim() == ".")
        })
    };
    for w in [120, 100, 90, 80] {
        let mut d = Demo::new(w, 30);
        d.app.ask_icons();
        let s = d.screen();
        assert!(!orphan(&s), "{s}");
        d.keys("n");
        d.open("backend");
        d.keys("i");
        d.type_text("draft");
        d.keys("ctrl+q");
        let s = d.screen();
        assert!(s.contains("Quit slakio?") && !orphan(&s), "{s}");
    }
}

#[test]
fn the_icons_question_keeps_its_keys_and_says_it_can_be_changed() {
    for w in [120, 80] {
        let mut d = Demo::new(w, 30);
        d.app.ask_icons();
        let s = d.screen();
        assert!(s.contains("y Yes, icons"), "the keys below the question at {w}:\n{s}");
        assert!(s.contains("change"), "how to change it later:\n{s}");
    }
}

#[test]
fn help_stays_on_the_hint_line_at_80_columns() {
    let mut d = Demo::new(80, 24);
    assert!(d.status_line().contains("? help"), "{}", d.status_line());
    d.open("long-threads");
    d.keys("g g enter");
    assert!(d.status_line().contains("? help"), "{}", d.status_line());
}

#[test]
fn the_list_panel_marks_the_workspace_with_its_band_and_mentions_only_in_red() {
    let d = tokyo(120, 40);
    let buf = d.buffer();
    let t = d.app.theme.clone();
    assert_eq!(buf[(2, 0)].symbol(), "▌", "the chip on the list panel's title");
    assert_eq!(Some(buf[(2, 0)].fg), t.workspace(d.app.model.workspaces()[0].color).fg, "the workspace's color");
    let chip = d.app.chip_bar().unwrap();
    let views = d.app.view_switcher().unwrap();
    for (bar, y) in [(chip, 0), (views, 1)] {
        for p in bar
            .pieces
            .iter()
            .filter(|p| matches!(p.part, slakio_tui::navbar::Part::Badge | slakio_tui::navbar::Part::Mark))
        {
            let c = &buf[(p.x + 1, y)];
            let mention = match p.item {
                Some(i) => d.app.view_unread(d.app.workspace(), slakio_tui::app::shell::View::ALL[i]).red(),
                None => p.text.trim().starts_with('@'),
            };
            assert_eq!(c.fg == t.error, mention, "{:?}: red for mentions only", p.text);
        }
    }
}

#[test]
fn the_which_key_popup_dims_the_screen_and_hides_the_hints() {
    let mut d = tokyo(120, 40);
    let before = d.buffer();
    let t0 = d.now;
    d.keys("space");
    d.now = t0 + WHICH_KEY_DELAY;
    d.app.on_tick(d.now);
    let after = d.buffer();
    let list = screen::list_parts(d.app.areas().list.unwrap()).rows;
    let (x, y) = (list.x + 3, list.y);
    assert_ne!(after[(x, y)].fg, before[(x, y)].fg, "the list behind the popup is dimmed");
    assert!(!d.status_line().contains("Enter open"), "{}", d.status_line());
}

#[test]
fn dividers_span_the_pane_and_use_one_color() {
    let mut d = tokyo(120, 40);
    d.open("long-threads");
    let main = d.pane(PaneKind::Conversation).unwrap();
    let i = main.messages().iter().position(|m| m.thread.is_some_and(|t| t.replies == 6)).unwrap();
    d.app.select_message(i);
    d.keys("enter");
    let thread = d.pane_area(PaneKind::Thread);
    let thread = thread.unwrap();
    let buf = d.buffer();
    let inner = screen::inner(thread);
    // The text area is inside one cell of padding: a rule fills it end to end.
    for y in inner.top()..inner.bottom() {
        let row: String = (inner.x + 1..inner.right() - 1).map(|x| buf[(x, y)].symbol()).collect();
        if row.contains("replies") || row.contains("2026") {
            assert!(row.starts_with('─') && row.ends_with('─'), "{row:?}");
        }
    }
    // The composer's divider: one color from tee to tee.
    let y = (inner.top()..inner.bottom()).find(|&y| buf[(thread.x, y)].symbol() == "├").unwrap();
    let colors: std::collections::HashSet<Color> =
        (thread.x..thread.right()).map(|x| &buf[(x, y)]).filter(|c| c.symbol() != " ").map(|c| c.fg).collect();
    assert_eq!(colors.len(), 1, "{colors:?}");
}

#[test]
fn a_short_status_line_keeps_the_place_tidy_and_apart_from_the_hints() {
    let mut d = Demo::new(80, 24);
    d.open("backend");
    let line = d.status_line();
    // The workspace's name gives way first (eight cells kept), never the place.
    assert!(
        line.contains("▌A compa")
            && line.contains(" › #backend")
            && line.contains("│ i write")
            && line.contains("? help"),
        "{line}"
    );
    assert!(!line.contains("#…"), "the place is cut only after the hints: {line}");
}

#[test]
fn a_view_of_a_later_version_hints_no_key_that_does_nothing_there() {
    let mut d = Demo::new(160, 40);
    d.keys("space a");
    let line = d.status_line();
    assert!(!line.contains("Enter open") && !line.contains("l peek") && line.contains("? help"), "{line}");
}

/// The blank cells between the status line's two sides, beyond the separator's own space.
fn gap(line: &str) -> usize {
    let body = line.trim_end();
    let at = body.find(" │ ").expect("the right side starts with a separator");
    body[..at].chars().rev().take_while(|c| *c == ' ').count()
}

#[test]
fn the_status_line_shortens_hints_before_the_name_and_wastes_no_room() {
    // At 120 columns the whole name stays: the hints of least worth make room for it.
    let d = Demo::new(120, 40);
    assert!(
        d.status_line().contains("▌A company › Home") && d.status_line().contains("│ Enter open"),
        "{}",
        d.status_line()
    );
    for w in [120u16, 100, 80] {
        let mut d = Demo::new(w, 30);
        let home = d.status_line();
        assert!(home.contains("▌A co"), "{w}: the name stays: {home}");
        assert!(home.contains("Enter open") && home.contains("? help"), "{w}: {home}");
        // Enter open is worth more than the peek.
        assert!(!home.contains("l peek") || home.contains("Enter open"), "{home}");
        d.open("backend");
        let pane = d.status_line();
        assert!(pane.contains("▌A co") && pane.contains("? help"), "{w}: {pane}");
        let hints: &[&str] = &[
            "Enter open",
            "l peek",
            "Tab next pane",
            ": commands",
            "i write",
            "k messages",
            "Esc list",
            "? help",
            "Space more",
        ];
        for line in [&home, &pane] {
            // Blanks left over are fewer than any hint that was dropped needs (with its ` · `),
            // and a cut name never sits beside blanks.
            let narrowest = hints
                .iter()
                .filter(|h| {
                    (line.as_str() == home.as_str())
                        == ["Enter open", "l peek", "Tab next pane", ": commands", "? help", "Space more"].contains(h)
                        || line.as_str() == pane.as_str() && !["Enter open", "l peek", ": commands"].contains(h)
                })
                .filter(|h| !line.contains(**h))
                .map(|h| h.chars().count() + 3)
                .min()
                .unwrap_or(usize::MAX);
            assert!(gap(line) < narrowest, "{w}: {line:?}");
            assert!(gap(line) == 0 || !line.contains('…'), "{w}: {line:?}");
        }
    }
}

#[test]
fn a_selected_reply_hints_how_to_reply_not_a_thread() {
    let mut d = Demo::new(160, 40);
    d.open("long-threads");
    d.keys("g g enter");
    d.keys("k");
    let line = d.status_line();
    assert!(line.contains("i reply · y copy") && !line.contains("Enter thread"), "{line}");
}

#[test]
fn the_icons_question_says_enter_chooses() {
    let mut d = Demo::new(80, 30);
    d.app.ask_icons();
    let s = d.screen();
    assert!(s.contains("· Enter choose"), "the footer whole: {s}");
}
