//! The look, not only the text: text snapshots cannot see an underline, a gray that hides text
//! or a panel blanked under the rail. These tests read every cell's style, in every built-in
//! theme and without color, and some screens are snapshots with their styles written beside
//! each row as theme token names (`screen_styled`).

#[path = "support/demo.rs"]
mod demo;

use demo::{Demo, mask_hangul};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use slakio_core::i18n::Lang;
use slakio_tui::app::Settings;
use slakio_tui::app::shell::Region;
use slakio_tui::screen;
use slakio_tui::theme::{Background, Kind, Theme, resolve};

fn themes() -> Vec<Theme> {
    vec![
        resolve("terminal", false, Background::Dark),
        resolve("dark", true, Background::Dark),
        resolve("tokyo-night", true, Background::Dark),
        resolve("tokyo-night", true, Background::Light),
        Theme::no_color(),
    ]
}

fn demo(theme: &Theme, w: u16, h: u16) -> Demo {
    let mut d = Demo::with(w, h, Lang::En, Settings::default());
    d.app.theme = theme.clone();
    d
}

/// The frames the checks look at: the list focused and not, a pane with a selected message
/// focused and not, the thread panel, the rail focused.
fn frames(theme: &Theme) -> Vec<(&'static str, Demo)> {
    let mut out = Vec::new();
    out.push(("home", demo(theme, 120, 40)));
    let mut d = demo(theme, 120, 40);
    d.open("backend");
    d.keys("k k");
    out.push(("pane selected", d));
    let mut d = demo(theme, 120, 40);
    d.open("long-threads");
    d.keys("g g enter");
    out.push(("thread", d));
    let mut d = demo(theme, 120, 40);
    d.open("backend");
    d.keys("k ctrl+h");
    out.push(("list after a pane", d));
    let mut d = demo(theme, 120, 40);
    d.keys("ctrl+h");
    out.push(("rail", d));
    out
}

// ① No selection is ever drawn underlined (only the own name of NO_COLOR is, by design).
#[test]
fn nothing_is_underlined() {
    for t in themes() {
        for (what, d) in frames(&t) {
            let buf = d.buffer();
            for (i, c) in buf.content().iter().enumerate() {
                assert!(!c.modifier.contains(Modifier::UNDERLINED), "{} {what}: cell {i} {:?}", t.name, c.symbol());
            }
        }
    }
}

/// The cells inside `panel` (below its title row), but the row `except`.
fn inside(buf: &Buffer, panel: Rect, except: Option<u16>) -> Vec<(u16, u16, Color, Modifier)> {
    let inner = screen::inner(panel);
    let mut out = Vec::new();
    for y in inner.top()..inner.bottom() {
        if Some(y) == except {
            continue;
        }
        for x in inner.left()..inner.right() {
            let c = &buf[(x, y)];
            out.push((x, y, c.fg, c.modifier));
        }
    }
    out
}

// ② The focus changes a panel's border and title, never the color of what is inside.
#[test]
fn unfocused_text_keeps_its_color() {
    for t in themes() {
        let mut d = demo(&t, 120, 40);
        d.keys("l");
        let list = d.app.areas().list.unwrap();
        let cursor_y = list.y + 1 + (d.app.shell.list_cursor - d.app.shell.list_top) as u16;
        assert_eq!(d.app.shell.focus, Region::List);
        let focused = inside(&d.buffer(), list, Some(cursor_y));
        d.keys("tab");
        assert_eq!(d.app.shell.focus, Region::Work);
        let unfocused = inside(&d.buffer(), list, Some(cursor_y));
        assert_eq!(focused, unfocused, "{}: the list's text changed with the focus", t.name);
    }
}

// ③ The overlay rail leaves the list drawn beside it.
#[test]
fn the_overlay_rail_leaves_the_list_visible() {
    for t in themes() {
        let mut d = demo(&t, 120, 40);
        d.keys("ctrl+h");
        let a = d.app.areas();
        let list = a.list.unwrap();
        let buf = d.buffer();
        let shown = (list.y + 1..list.bottom() - 1)
            .filter(|&y| (a.rail.right()..list.right() - 1).any(|x| buf[(x, y)].symbol() != " "))
            .count();
        assert!(shown > 10, "{}: only {shown} rows of the list show beside the rail", t.name);
    }
}

// ④ Exactly one panel has the accent border: the one with the focus.
#[test]
fn one_accent_border() {
    for t in themes().into_iter().filter(|t| t.kind != Kind::NoColor) {
        for (what, d) in frames(&t) {
            let buf = d.buffer();
            let corners = buf.content().iter().filter(|c| c.symbol() == "╭" && c.fg == t.accent).count();
            assert_eq!(corners, 1, "{} {what}", t.name);
        }
        // Hovered, the rail opens without the focus: no accent on it.
        let mut d = demo(&t, 120, 40);
        d.mouse(ratatui::crossterm::event::MouseEventKind::Moved, 1, 5);
        let buf = d.buffer();
        assert_ne!(buf[(0, 0)].fg, t.accent, "{}: a hovered rail is not focused", t.name);
    }
}

// ⑤ The focused selection is a bar across the whole row (a count pill keeps its own color).
#[test]
fn the_selection_is_a_bar_across_the_row() {
    for t in themes().into_iter().filter(|t| t.kind != Kind::NoColor) {
        let d = demo(&t, 120, 40);
        let list = d.app.areas().list.unwrap();
        let inner = screen::inner(list);
        let y = inner.y + (d.app.shell.list_cursor - d.app.shell.list_top) as u16;
        let buf = d.buffer();
        for x in inner.left()..inner.right() {
            let c = &buf[(x, y)];
            assert!(c.bg == t.selection || c.bg == t.error, "{}: cell {x} {:?}", t.name, c.bg);
        }
        // Without the focus: the faint bar, or a gutter mark.
        let mut d = d;
        d.keys("l tab");
        let buf = d.buffer();
        match t.kind {
            Kind::Truecolor => assert_eq!(buf[(inner.x + 5, y)].bg, t.cursor_line, "{}", t.name),
            _ => assert_eq!(buf[(inner.x, y)].symbol(), "▎", "{}", t.name),
        }
    }
    // Without color: reversed.
    let d = demo(&Theme::no_color(), 120, 40);
    let inner = screen::inner(d.app.areas().list.unwrap());
    let y = inner.y + d.app.shell.list_cursor as u16;
    assert!(d.buffer()[(inner.x + 4, y)].modifier.contains(Modifier::REVERSED));
}

// ⑥ No text in the color of its own background (gray on a gray bar).
#[test]
fn no_text_hides_in_its_background() {
    for t in themes() {
        for (what, d) in frames(&t) {
            let buf = d.buffer();
            for (i, c) in buf.content().iter().enumerate() {
                let visible = c.symbol().trim().is_empty() || c.fg == Color::Reset || c.fg != c.bg;
                assert!(visible, "{} {what}: cell {i} {:?} is {:?} on {:?}", t.name, c.symbol(), c.fg, c.bg);
            }
        }
    }
}

/// The name of the token behind a color of `t`.
fn token(t: &Theme, c: Color, fg: bool) -> Option<&'static str> {
    let named = [
        (t.accent, "acc"),
        (t.accent_warm, "warm"),
        (t.border, "bdr"),
        (t.fg_muted, "mut"),
        (t.fg_dim, "dim"),
        (t.error, "err"),
        (t.warning, "warn"),
        (t.selection, "sel"),
        (t.cursor_line, "cl"),
        (t.range, "rng"),
        (t.surface, "surf"),
        (t.surface_alt, "surf2"),
        (t.mode_normal, "normal"),
        (t.mode_fg, "on"),
    ];
    if c == t.fg && fg || c == t.bg && !fg || c == Color::Reset {
        return None;
    }
    let workspace = t.workspaces.contains(&c).then_some("ws");
    Some(named.iter().find(|(x, _)| *x == c).map(|(_, n)| *n).or(workspace).unwrap_or(if fg { "fg?" } else { "bg?" }))
}

/// The screen as text, each row followed by its styled runs: `[acc 0..3]`, `[sel/fg 5..30]`,
/// `[mut B 12..20]` (B bold, R reversed, D dim), in theme token names.
fn screen_styled(d: &Demo) -> String {
    let t = &d.app.theme;
    let buf = d.buffer();
    let (w, h) = (d.app.size.width, d.app.size.height);
    let text = mask_hangul(&d.screen());
    let mut out = String::new();
    for (y, line) in text.lines().enumerate().take(usize::from(h)) {
        out.push_str(line);
        out.push('\n');
        let mut runs: Vec<(String, u16, u16)> = Vec::new();
        for x in 0..w {
            let c = &buf[(x, y as u16)];
            let mut name = String::new();
            if let Some(n) = token(t, c.fg, true) {
                name.push_str(n);
            }
            if let Some(n) = token(t, c.bg, false) {
                name.push_str(&format!("/{n}"));
            }
            for (m, s) in [(Modifier::BOLD, " B"), (Modifier::REVERSED, " R"), (Modifier::DIM, " D")] {
                if c.modifier.contains(m) {
                    name.push_str(s);
                }
            }
            match runs.last_mut() {
                Some((n, _, end)) if *n == name && *end == x => *end = x + 1,
                _ => runs.push((name, x, x + 1)),
            }
        }
        let shown: Vec<String> =
            runs.into_iter().filter(|(n, _, _)| !n.is_empty()).map(|(n, a, b)| format!("[{n} {a}..{b}]")).collect();
        if !shown.is_empty() {
            out.push_str("    ");
            out.push_str(&shown.join(" "));
            out.push('\n');
        }
    }
    out
}

#[test]
fn styled_snapshots_in_tokyo_night() {
    let t = resolve("tokyo-night", true, Background::Dark);
    let d = demo(&t, 120, 40);
    insta::assert_snapshot!("styled_demo_home_120x40", screen_styled(&d));
    let mut d = demo(&t, 120, 40);
    d.open("backend");
    insta::assert_snapshot!("styled_demo_open_120x40", screen_styled(&d));
    let mut d = demo(&t, 120, 40);
    d.open("long-threads");
    d.keys("g g enter");
    insta::assert_snapshot!("styled_case2_thread_120x40", screen_styled(&d));
    let mut d = demo(&t, 120, 40);
    d.keys("ctrl+h");
    insta::assert_snapshot!("styled_rail_overlay_focus_120x40", screen_styled(&d));
    let mut d = demo(&t, 120, 40);
    d.mouse(ratatui::crossterm::event::MouseEventKind::Moved, 1, 5);
    insta::assert_snapshot!("styled_rail_overlay_hover_120x40", screen_styled(&d));
    let mut d = demo(&resolve("terminal", false, Background::Dark), 80, 24);
    d.keys("ctrl+h");
    insta::assert_snapshot!("styled_rail_overlay_focus_terminal_80x24", screen_styled(&d));
}
