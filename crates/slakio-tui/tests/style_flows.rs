//! The look, not only the text: text snapshots cannot see an underline, a gray that hides text
//! or a panel blanked under a popup. These tests read every cell's style, in every built-in
//! theme and without color, and some screens are snapshots with their styles written beside
//! each row as theme token names (`screen_styled`).

#[path = "support/demo.rs"]
mod demo;

use demo::{Demo, mask_hangul};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use slakio_core::i18n::Lang;
use slakio_tui::app::shell::View;
use slakio_tui::app::{Focus, PaneKind, Settings};
use slakio_tui::screen;
use slakio_tui::tabbar::Part;
use slakio_tui::theme::{BUILTINS, Background, Kind, Theme, resolve};

/// Every built-in theme (the terminal's colors on a dark background), and none.
fn themes() -> Vec<Theme> {
    let mut out: Vec<Theme> = BUILTINS.iter().map(|t| resolve(t.name, true, Background::Dark)).collect();
    out.push(Theme::no_color());
    out
}

fn demo(theme: &Theme, w: u16, h: u16) -> Demo {
    let mut d = Demo::with(w, h, Lang::En, Settings::default());
    d.app.theme = theme.clone();
    d
}

/// The frames the checks look at: the list focused and not, a pane with a selected message
/// focused and not, the thread panel, the view switcher focused.
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
    d.keys("ctrl+r");
    out.push(("view switcher", d));
    out.push(("tabs", tabs(theme)));
    out
}

/// Three tabs, the second shown, the first with mentions (a badge on the bar).
fn tabs(theme: &Theme) -> Demo {
    let mut d = demo(theme, 120, 40);
    d.open("incidents");
    d.open_in_tab("backend");
    d.open_in_tab("general");
    d.keys("space 2");
    d
}

// The tab bar: the tab shown raised and bold, the others muted, `×` quiet; never underlined.
#[test]
fn the_tab_shown_stands_out_on_the_bar() {
    for t in themes() {
        let d = tabs(&t);
        let bar = d.app.tab_bar().expect("three tabs: the bar shows");
        let buf = d.buffer();
        let cell = |tab: usize, part: Part| {
            let p = bar.pieces.iter().find(|p| p.tab == tab && p.part == part).expect("drawn");
            buf[(p.x, bar.area.y)].clone()
        };
        let (shown, other) = (cell(1, Part::Title), cell(2, Part::Title));
        assert!(shown.modifier.contains(Modifier::BOLD) && !other.modifier.contains(Modifier::BOLD), "{}", t.name);
        match t.kind {
            Kind::NoColor => assert!(shown.modifier.contains(Modifier::REVERSED), "{}", t.name),
            Kind::Truecolor => {
                assert_eq!((shown.fg, shown.bg), (t.fg, t.raised()), "{}", t.name);
                assert_eq!(other.fg, t.fg_muted, "{}", t.name);
                assert_eq!(cell(1, Part::Number).fg, t.accent, "{}", t.name);
                assert_eq!(cell(2, Part::Close).fg, t.fg_dim, "{}", t.name);
            }
            Kind::Ansi => assert_eq!(other.fg, t.fg_muted, "{}", t.name),
        }
        let p = bar.pieces.iter().find(|p| p.tab == 0 && p.part == Part::Badge).expect("a badge");
        let badge = &buf[(p.x + 1, bar.area.y)];
        // #incidents mentions the user: `@n` in the mention color.
        assert!(badge.symbol() == "@" && badge.modifier.contains(Modifier::BOLD), "{}: {badge:?}", t.name);
        if t.kind != Kind::NoColor {
            assert_eq!(badge.fg, t.error, "{}", t.name);
        }
    }
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

/// The cells inside `panel` (below its title row), but the row `except` and a rule joined to the
/// border (it is the border's).
fn inside(buf: &Buffer, panel: Rect, except: Option<u16>) -> Vec<(u16, u16, Color, Modifier)> {
    let inner = screen::inner(panel);
    let mut out = Vec::new();
    for y in inner.top()..inner.bottom() {
        if Some(y) == except || buf[(panel.x, y)].symbol() == "├" {
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
        let cursor_y = screen::list_parts(list).rows.y + (d.app.list_cursor() - d.app.list_top()) as u16;
        assert_eq!(d.app.focus(), Focus::List);
        let focused = inside(&d.buffer(), list, Some(cursor_y));
        d.keys("tab");
        assert_eq!(d.focused_kind(), Some(PaneKind::Conversation));
        let unfocused = inside(&d.buffer(), list, Some(cursor_y));
        assert_eq!(focused, unfocused, "{}: the list's text changed with the focus", t.name);
    }
}

// ③ The view switcher (the list panel's first row) and the tab bar (the work area's) are both on
// the background; the view shown is raised like the tab shown, the others muted. No row is on the
// status line's surface but the status line.
#[test]
fn the_view_switcher_and_the_tab_bar_read_alike_on_the_background() {
    for t in themes().into_iter().filter(|t| t.kind == Kind::Truecolor) {
        let d = tabs(&t);
        let buf = d.buffer();
        let tabs_row = d.app.areas().tabs.unwrap().y;
        assert_eq!(buf[(119, tabs_row)].bg, t.bg, "{}: the tab bar on the background", t.name);
        let bar = d.app.view_switcher().unwrap();
        let y = bar.area.y;
        let home = bar.pieces.iter().find(|p| p.part == slakio_tui::navbar::Part::Label && p.item == Some(0)).unwrap();
        let cell = &buf[(home.x, y)];
        assert_eq!((cell.bg, cell.modifier.contains(Modifier::BOLD)), (t.raised(), true), "{}: Home is shown", t.name);
        let files = bar.pieces.iter().find(|p| p.part == slakio_tui::navbar::Part::Label && p.item == Some(3)).unwrap();
        assert_eq!((buf[(files.x, y)].fg, buf[(files.x, y)].bg), (t.fg_muted, t.bg), "{}", t.name);
        for x in 0..120 {
            assert_ne!(buf[(x, 0)].bg, t.surface, "{}: no top bar on the surface (column {x})", t.name);
        }
    }
}

// ④ Exactly one panel has the accent border: the one with the focus.
#[test]
fn one_accent_border() {
    for t in themes().into_iter().filter(|t| t.kind != Kind::NoColor) {
        for (what, d) in frames(&t) {
            let buf = d.buffer();
            let corners = buf.content().iter().filter(|c| c.symbol() == "╭" && c.fg == t.accent).count();
            // The view switcher is the list panel's: with the focus there, the list's border.
            assert_eq!(corners, 1, "{} {what}", t.name);
        }
    }
}

// ⑤ The focused selection is a bar across the whole row (a count pill keeps its own color).
#[test]
fn the_selection_is_a_bar_across_the_row() {
    for t in themes().into_iter().filter(|t| t.kind != Kind::NoColor) {
        let d = demo(&t, 120, 40);
        let list = d.app.areas().list.unwrap();
        let inner = screen::list_parts(list).rows;
        let y = inner.y + (d.app.list_cursor() - d.app.list_top()) as u16;
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
    let inner = screen::list_parts(d.app.areas().list.unwrap()).rows;
    let y = inner.y + d.app.list_cursor() as u16;
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
    // An avatar chip: its hue (the initials on a tint) or its background.
    let chip = (0..t.avatars.len()).any(|i| t.avatar(i).bg == Some(c));
    if !fg && (chip || t.avatars.contains(&c)) || fg && t.avatars.contains(&c) {
        return Some("av");
    }
    if !fg && t.avatar_group().bg == Some(c) {
        return Some("chip");
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
    d.keys("ctrl+r l");
    insta::assert_snapshot!("styled_nav_focus_120x40", screen_styled(&d));
    let mut d = demo(&t, 120, 40);
    d.keys("space W");
    insta::assert_snapshot!("styled_switcher_120x40", screen_styled(&d));
    let mut d = demo(&resolve("terminal", false, Background::Dark), 80, 24);
    d.keys("ctrl+r");
    insta::assert_snapshot!("styled_nav_focus_terminal_80x24", screen_styled(&d));
    let mut d = demo(&Theme::no_color(), 80, 24);
    d.keys("ctrl+r");
    insta::assert_snapshot!("styled_nav_focus_no_color_80x24", screen_styled(&d));
    insta::assert_snapshot!("styled_tabs_120x40", screen_styled(&tabs(&t)));
}

/// A conversation of workspace A, not muted, that `pick` takes.
fn conversation_where(d: &Demo, pick: impl Fn(&slakio_core::model::Conversation) -> bool) -> String {
    (0..)
        .map(|i| d.app.model.conversation(i))
        .find(|c| c.workspace.as_str() == "TDEMOA" && !c.muted && pick(c))
        .map(|c| c.name.line().as_str().to_string())
        .expect("the demo world has one")
}

// A tab not shown says what the list says of its conversation: `@n` mentions in the mention
// color, `●n` a DM's unread messages, `●` a channel's, both plain bold.
#[test]
fn a_tab_badge_matches_the_list_pill_and_only_mentions_are_red() {
    let t = resolve("tokyo-night", true, Background::Dark);
    let probe = demo(&t, 160, 40);
    let mention = conversation_where(&probe, |c| c.mentions > 0 && !c.is_dm());
    let quiet = conversation_where(&probe, |c| c.unread > 0 && c.mentions == 0 && !c.is_dm());
    let dm = conversation_where(&probe, |c| c.unread > 0 && c.mentions == 0 && c.is_dm());
    let counts = |name: &str| {
        let c = (0..).map(|i| probe.app.model.conversation(i)).find(|c| c.name.line().as_str() == name).unwrap();
        (c.mentions, c.unread)
    };
    let mut d = demo(&t, 200, 40);
    d.open(&mention);
    d.keys("space d");
    d.list_cursor_on_view(&dm);
    d.keys("t");
    d.open_in_tab(&quiet);
    d.open_in_tab("backend");
    let bar = d.app.tab_bar().unwrap();
    let buf = d.buffer();
    let badge = |tab: usize| {
        let p = bar.pieces.iter().find(|p| p.tab == tab && p.part == Part::Badge).expect("a badge");
        (p.text.trim().to_string(), buf[(p.x + 1, bar.area.y)].clone())
    };
    let (text, cell) = badge(0);
    assert_eq!(text, format!("@{}", counts(&mention).0), "mentions, as the list's pill counts them");
    assert_eq!((cell.fg, cell.modifier.contains(Modifier::BOLD)), (t.error, true));
    let (text, cell) = badge(1);
    assert_eq!(text, format!("●{}", counts(&dm).1), "a DM's unread messages, as its pill");
    assert_eq!((cell.fg, cell.modifier.contains(Modifier::BOLD)), (t.fg, true), "not the mention color");
    let (text, cell) = badge(2);
    assert_eq!(text, "●", "a channel's unread: a mark, no count (the list has none)");
    assert_eq!(cell.fg, t.fg);
}

// The `‹` / `›` marks take the strongest mark of the tabs they hide: a mention's color, else
// bold for unread, else the plain accent.
#[test]
fn the_overflow_marks_carry_what_the_hidden_tabs_hold() {
    let t = resolve("tokyo-night", true, Background::Dark);
    let probe = demo(&t, 80, 24);
    let mention = conversation_where(&probe, |c| c.mentions > 0 && !c.is_dm());
    let mut d = demo(&t, 80, 24);
    d.open(&mention);
    for name in ["incidents", "general", "random", "deploys", "long-threads", "big-history", "alerts"] {
        if name != mention {
            d.open_in_tab(name);
        }
    }
    let bar = d.app.tab_bar().unwrap();
    let (x, _) = bar.left.expect("tabs hidden on the left");
    let cell = d.buffer()[(x, bar.area.y)].clone();
    assert_eq!((cell.symbol(), cell.fg), ("‹", t.error), "a hidden tab mentions the user");
    d.keys("space 1");
    let bar = d.app.tab_bar().unwrap();
    let (x, _) = bar.right.expect("tabs hidden on the right");
    let cell = d.buffer()[(x, bar.area.y)].clone();
    assert_eq!(cell.symbol(), "›");
    assert_ne!(cell.fg, t.error, "the tabs on the right mention nobody");
}

// The view shown and the tab shown share one style in every theme: with truecolor one raised
// background that stands out of the background, without it reversed and bold; `▾` on the chip
// is quiet.
#[test]
fn the_view_shown_and_the_tab_shown_read_alike_in_every_theme() {
    for t in themes() {
        let d = tabs(&t);
        let buf = d.buffer();
        let bar = d.app.view_switcher().unwrap();
        let chip = d.app.chip_bar().unwrap();
        let tabs_bar = d.app.tab_bar().unwrap();
        let home = bar.pieces.iter().find(|p| p.part == slakio_tui::navbar::Part::Label && p.item == Some(0)).unwrap();
        let caret = chip.pieces.iter().find(|p| p.part == slakio_tui::navbar::Part::Caret).unwrap();
        let name = chip.pieces.iter().find(|p| p.part == slakio_tui::navbar::Part::Name).unwrap();
        let shown_tab = tabs_bar.pieces.iter().find(|p| p.tab == 1 && p.part == Part::Title).unwrap();
        let (h, c) = (&buf[(home.x, bar.area.y)], &buf[(caret.x + 1, chip.area.y)]);
        let st = &buf[(shown_tab.x, tabs_bar.area.y)];
        assert_eq!((h.bg, h.modifier), (st.bg, st.modifier), "{}: one style for what is shown", t.name);
        match t.kind {
            Kind::Truecolor => {
                assert_eq!(h.bg, t.raised(), "{}: the raised background", t.name);
                assert!(t.raised() != t.bg, "{}: it stands out", t.name);
                assert_eq!(c.fg, t.fg_muted, "{}: the caret is quiet", t.name);
            }
            _ => assert!(h.modifier.contains(Modifier::REVERSED | Modifier::BOLD), "{}", t.name),
        }
        // Nothing between the name shown and its caret: the other workspaces' marks follow it.
        assert_eq!(name.x + name.width, caret.x, "{}", t.name);
    }
}

// A pill is red only for a real mention; a DM's pill counts its unread messages, neutral
// without a mention. Its counts add up to the view switcher's DMs `●n`. Nothing is red anywhere —
// list, tabs, view switcher — without a mention behind it.
#[test]
fn red_means_a_mention_and_the_dm_counts_add_up() {
    use slakio_tui::app::model::Row;
    let t = resolve("tokyo-night", true, Background::Dark);
    let mut d = demo(&t, 160, 60);
    d.keys("space d");
    let list = screen::list_parts(d.app.areas().list.unwrap()).rows;
    let buf = d.buffer();
    let mut sum = 0u32;
    for (k, row) in d.app.list_rows().iter().enumerate().skip(d.app.list_top()).take(usize::from(list.height)) {
        let Row::Conversation(i) = row else { continue };
        let c = d.app.model.conversation(*i);
        let y = list.y + (k - d.app.list_top()) as u16;
        let cells: Vec<&ratatui::buffer::Cell> = (list.x..list.right()).map(|x| &buf[(x, y)]).collect();
        let text: String = cells.iter().map(|c| c.symbol()).collect();
        let red = cells.iter().any(|cell| cell.bg == t.error);
        assert_eq!(red, c.mentions > 0, "{text}: red only with a mention");
        if c.unread > 0 && !c.muted {
            assert!(text.trim_end().ends_with(&format!(" {}", c.unread)), "{text}: its unread count");
            sum += c.unread;
        }
    }
    let rows_all = d.app.list_rows().len();
    assert!(rows_all <= usize::from(list.height), "every DM row is on screen for the sum");
    let views = d.app.view_switcher().unwrap();
    let row: String = (0..views.area.right()).map(|x| buf[(x, views.area.y)].symbol().to_string()).collect();
    assert!(row.contains(&format!("DMs ●{sum}")), "{row}");
    // Tabs and the view switcher: red only where a mention is.
    let mut d = demo(&t, 160, 40);
    d.keys("space d");
    let quiet = (0..)
        .map(|i| d.app.model.conversation(i))
        .find(|c| c.workspace.as_str() == "TDEMOA" && c.is_dm() && c.unread > 0 && c.mentions == 0 && !c.muted)
        .unwrap()
        .name
        .line()
        .as_str()
        .to_string();
    d.list_cursor_on_view(&quiet);
    d.keys("t");
    d.open_in_tab("backend");
    let buf = d.buffer();
    let tabs = d.app.tab_bar().unwrap();
    let badge = tabs.pieces.iter().find(|p| p.tab == 0 && p.part == Part::Badge).expect("the DM tab's mark");
    assert!(badge.text.trim().starts_with('●'), "{:?}: a DM's unread, never @", badge.text);
    assert_ne!(buf[(badge.x + 1, tabs.area.y)].fg, t.error, "no mention: not red");
    for bar in [d.app.chip_bar().unwrap(), d.app.view_switcher().unwrap()] {
        let y = bar.area.y;
        let marks = bar.pieces.iter();
        for p in marks.filter(|p| matches!(p.part, slakio_tui::navbar::Part::Badge | slakio_tui::navbar::Part::Mark)) {
            let red = buf[(p.x + 1, y)].fg == t.error;
            let mention = match p.item {
                Some(i) => d.app.view_unread(d.app.workspace(), View::ALL[i]).red(),
                None => p.text.trim().starts_with('@'),
            };
            assert_eq!(red, mention, "{:?}: red only with a mention behind it", p.text);
        }
    }
}

// Without truecolor a mention on the view switcher is bold in the mention color, out of the
// reversed view shown; a light theme raises the view shown clearly.
#[test]
fn marks_on_the_view_switcher_read_without_truecolor_and_light_themes_raise_clearly() {
    let t = resolve("terminal", false, Background::Dark);
    let d = demo(&t, 160, 40);
    let buf = d.buffer();
    let bar = d.app.view_switcher().unwrap();
    let activity = bar.pieces.iter().find(|p| p.part == slakio_tui::navbar::Part::Badge && p.item == Some(2)).unwrap();
    let cell = &buf[(activity.x + 1, bar.area.y)];
    assert!(cell.modifier.contains(Modifier::BOLD) && !cell.modifier.contains(Modifier::REVERSED), "Activity @n");
    for name in ["light", "catppuccin-latte"] {
        let t = resolve(name, true, Background::Light);
        assert_eq!(t.raised(), t.selection, "{name}: the selection's color, clearly apart from the background");
    }
}
