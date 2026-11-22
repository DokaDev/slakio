use super::*;
use crate::images::graphics;
use crate::theme::{Background, Selection, TOKYO_NIGHT_NIGHT};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use slakio_core::theme_file::{COLOR_TOKENS, STYLE_TOKENS};

const DARK_TRUECOLOR: Look = Look { truecolor: true, background: Background::Dark, no_color: false };

fn rgb(n: u32) -> Color {
    Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)
}

/// A file extending `tokyo-night` that sets every token: colour `i` is `#0a00<i>`, style `i` is
/// `#c000<i>` on `#d000<i>`, bold and italic.
fn every_token() -> String {
    let mut s = String::from("extends = \"tokyo-night\"\n[colors]\n");
    for (i, t) in COLOR_TOKENS.iter().enumerate() {
        s.push_str(&format!("{t} = \"#0a00{i:02x}\"\n"));
    }
    s.push_str("workspaces = [\"#e00001\", \"#e00002\"]\navatars = [\"#f00001\"]\navatar_tint = 0\n");
    s.push_str("dim = { toward = \"#000000\", keep = 30 }\n[styles]\n");
    for (i, t) in STYLE_TOKENS.iter().enumerate() {
        s.push_str(&format!(
            "{t} = {{ fg = \"#c000{i:02x}\", bg = \"#d000{i:02x}\", modifiers = [\"bold\", \"italic\"] }}\n"
        ));
    }
    s
}

fn theme(text: &str, look: Look) -> Theme {
    from_spec("mine", &theme_file::parse(text).unwrap(), look)
}

fn style_of(i: usize) -> Style {
    Style::new()
        .fg(rgb(0xc00000 + i as u32))
        .bg(rgb(0xd00000 + i as u32))
        .add_modifier(Modifier::BOLD | Modifier::ITALIC)
}

#[test]
fn every_token_of_a_file_lands_where_it_is_drawn() {
    let t = theme(&every_token(), DARK_TRUECOLOR);
    assert_eq!((t.name.as_ref(), t.kind), ("mine", Kind::Truecolor));
    let colors = [
        t.bg,
        t.surface,
        t.surface_alt,
        t.border,
        t.accent,
        t.accent_warm,
        t.fg,
        t.fg_muted,
        t.fg_dim,
        t.success,
        t.warning,
        t.error,
        t.selection,
        t.cursor_line,
        t.range,
        t.mode_normal,
        t.mode_insert,
        t.mode_visual,
        t.mode_command,
        t.mode_fg,
    ];
    assert_eq!(colors.len(), COLOR_TOKENS.len());
    for (i, c) in colors.iter().enumerate() {
        assert_eq!(*c, rgb(0x0a0000 + i as u32), "{}", COLOR_TOKENS[i]);
    }
    assert_eq!(t.workspaces.as_ref(), [rgb(0xe00001), rgb(0xe00002)]);
    assert_eq!(t.avatars.as_ref(), [rgb(0xf00001)]);
    assert_eq!(t.avatar_tint, 0);
    assert_eq!(t.dim, Dim::Blend { toward: rgb(0), keep: 30 });
    let at = |token: &str| style_of(STYLE_TOKENS.iter().position(|t| *t == token).unwrap());
    assert_eq!(t.link(), at("link"));
    assert_eq!(t.mention(true), at("mention"));
    assert_eq!(t.mention(false), at("mention"));
    assert_eq!(t.badge(), at("badge"));
    assert_eq!(t.current(), at("current"));
    assert_eq!(t.code(), at("code"));
    assert_eq!(t.reaction(), at("reaction"));
    assert_eq!(t.reaction_mine(), at("reaction_mine"));
    assert_eq!(t.tab(true), at("tab_active"));
    assert_eq!(t.tab(false), t.muted().bg(t.bg), "only the tab shown");
    assert_eq!(t.tab_badge(true, true), at("tab_badge"));
    assert_eq!(t.status_line(), at("status_line"));
    assert_eq!(t.hint(), at("hint"));
    assert_eq!(t.search_match(), at("search_match"));
    assert_eq!(t.selection_style(Selection::Focused), Some(at("selection")));
    assert_eq!(t.selection_style(Selection::Unfocused), Some(at("cursor_line")));
    // An avatar chip of the file's own colour, solid (no tint) under `mode_fg`.
    assert_eq!(t.avatar(3), Style::new().add_modifier(Modifier::BOLD).fg(t.mode_fg).bg(rgb(0xf00001)));
}

#[test]
fn a_file_of_one_token_is_its_base_but_for_that_token() {
    let t = theme("extends = \"tokyo-night\"\n[colors]\naccent = \"red\"\n", DARK_TRUECOLOR);
    let base = TOKYO_NIGHT_NIGHT;
    assert_eq!(t.accent, Color::Red);
    assert_eq!(Theme { name: base.name.clone(), accent: base.accent, ..t.clone() }, base, "the rest is the base's");
    assert_eq!(t.kind, Kind::Truecolor, "the base draws with 24-bit colour");
    // Without `extends`: the terminal's colours, kind and all.
    let t = theme("[colors]\naccent = \"bright-blue\"\n", DARK_TRUECOLOR);
    assert_eq!((t.kind, t.accent, t.bg), (Kind::Ansi, Color::LightBlue, Color::Reset));
    assert_eq!(theme("[colors]\nfg = \"#102030\"\n", DARK_TRUECOLOR).kind, Kind::Truecolor, "an #rrggbb colour");
    // A family by the terminal's background.
    let light = Look { background: Background::Light, ..DARK_TRUECOLOR };
    let day = theme("extends = \"tokyo-night\"\n", light);
    assert_eq!(day.bg, crate::theme::TOKYO_NIGHT_DAY.bg);
}

#[test]
fn a_custom_selection_and_dim_leave_every_picture_cell_as_it_is() {
    let t = theme(
        "extends = \"tokyo-night\"\n[colors]\ndim = { toward = \"#000000\", keep = 30 }\n\
         [styles]\nselection = { fg = \"#ff0000\", bg = \"#00ff00\", modifiers = [\"reversed\"] }\n\
         cursor_line = { fg = \"#ff0000\", bg = \"#00ff00\", modifiers = [\"reversed\"] }\n",
        DARK_TRUECOLOR,
    );
    for how in [Selection::Focused, Selection::Unfocused] {
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "text", t.text());
        let picture = graphics::color(0x0012_3456);
        buf.set_string(4, 0, graphics::row(0, 4), Style::new().fg(picture));
        let before: Vec<_> = (4..8).map(|x| buf[(x, 0)].clone()).collect();
        t.paint_selection(&mut buf, Rect::new(0, 0, 10, 1), how);
        for x in 0..4 {
            let c = &buf[(x, 0)];
            assert_eq!((c.fg, c.bg), (rgb(0xff0000), rgb(0x00ff00)), "{how:?}: text takes the style");
            assert!(c.modifier.contains(Modifier::REVERSED), "{how:?}");
        }
        t.dim_area(&mut buf, area);
        let after: Vec<_> = (4..8).map(|x| buf[(x, 0)].clone()).collect();
        assert_eq!(before, after, "{how:?}: a picture's cells keep symbol, colour (its id) and modifiers");
        assert!(after.iter().all(|c| graphics::is_placeholder(c.symbol()) && c.fg == picture));
        assert_ne!(buf[(0, 0)].fg, rgb(0xff0000), "{how:?}: the text is dimmed");
    }
}

#[test]
fn no_color_wins_over_a_theme_file() {
    let dir = std::env::temp_dir().join(format!("slakio-custom-theme-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("mine.toml"), every_token()).unwrap();
    let no_color = Look { no_color: true, ..DARK_TRUECOLOR };
    let t = load("mine", no_color, Some(&dir)).unwrap();
    assert_eq!((t.kind, t.name.as_ref()), (Kind::NoColor, "no-color"));
    assert_eq!(t, Theme::no_color());
    assert_eq!(load("mine", DARK_TRUECOLOR, Some(&dir)).unwrap().name, "mine");
    // A file that cannot be used is still said under NO_COLOR.
    std::fs::write(dir.join("bad.toml"), "[colors]\naccent = \"nope\"\n").unwrap();
    assert!(matches!(load("bad", no_color, Some(&dir)), Err(ThemeError::File { line: Some(2), .. })));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn built_in_names_never_read_a_file_and_unknown_names_say_so() {
    assert_eq!(load("Nord", DARK_TRUECOLOR, None).unwrap().name, "nord");
    assert!(matches!(load("mine", DARK_TRUECOLOR, None), Err(ThemeError::Unknown { dir: None, .. })));
    assert!(matches!(load("../x", DARK_TRUECOLOR, None), Err(ThemeError::BadName(_))));
}

#[test]
fn the_selected_row_of_a_panel_without_focus_shows_on_a_terminal_file_that_names_a_hex_colour() {
    // No `extends`: the base is `terminal`, whose bar colour is the terminal's own.
    for text in ["[colors]\naccent = \"#ff8800\"\n", "[colors]\ndim = { toward = \"#000000\", keep = 50 }\n"] {
        let t = theme(text, DARK_TRUECOLOR);
        assert_eq!(t.kind, Kind::Truecolor, "{text}");
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "general", Style::new().fg(t.fg));
        t.paint_selection(&mut buf, area, Selection::Unfocused);
        let bar = (0..10).any(|x| buf[(x, 0)].bg != Color::Reset);
        assert!(bar || buf[(0, 0)].symbol() == crate::theme::GUTTER, "{text}: the row is marked");
    }
    // A bar of its own: no gutter on top of it.
    let t = theme("extends = \"tokyo-night\"\n", DARK_TRUECOLOR);
    let area = Rect::new(0, 0, 10, 1);
    let mut buf = Buffer::empty(area);
    t.paint_selection(&mut buf, area, Selection::Unfocused);
    assert_eq!((buf[(0, 0)].bg, buf[(0, 0)].symbol()), (t.cursor_line, " "));
}
