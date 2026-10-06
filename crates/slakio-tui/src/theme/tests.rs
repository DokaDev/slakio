use super::*;
use ratatui::buffer::Buffer;

fn luminance(c: Color) -> f64 {
    let Color::Rgb(r, g, b) = c else { panic!("{c:?} is not RGB") };
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

fn colors(t: &Theme) -> Vec<Color> {
    let mut v = vec![
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
    v.extend(t.workspaces);
    v
}

/// The built-in themes that draw with 24-bit colors (the contrast checks are about RGB).
fn truecolor() -> impl Iterator<Item = &'static Theme> {
    BUILTINS.iter().copied().filter(|t| t.kind == Kind::Truecolor)
}

/// A theme for a dark background (its body text is lighter than its background).
fn dark(t: &Theme) -> bool {
    luminance(t.fg) > luminance(t.bg)
}

/// WCAG 2 contrast: body text 4.5:1 on every surface it is drawn on (also on the selection and
/// the cursor line), muted text and marks 3:1, the pills and the mode badges 4.5:1.
#[test]
fn every_truecolor_theme_keeps_text_readable() {
    assert!(truecolor().count() >= 11, "every built-in but the terminal's own colors");
    for t in truecolor() {
        let n = t.name;
        for bg in [t.bg, t.surface, t.surface_alt, t.selection, t.cursor_line, t.range] {
            assert!(contrast(t.fg, bg) >= 4.5, "{n}: body text on {bg:?} {:.2}", contrast(t.fg, bg));
        }
        for bg in [t.bg, t.surface, t.surface_alt] {
            assert!(contrast(t.fg_muted, bg) >= 3.0, "{n}: muted on {bg:?} {:.2}", contrast(t.fg_muted, bg));
            for c in [t.accent, t.accent_warm, t.success, t.warning, t.error] {
                assert!(contrast(c, bg) >= 3.0, "{n}: {c:?} on {bg:?} {:.2}", contrast(c, bg));
            }
        }
        assert!(contrast(t.mode_fg, t.error) >= 4.5, "{n}: badge {:.2}", contrast(t.mode_fg, t.error));
        for m in [t.mode_normal, t.mode_insert, t.mode_visual, t.mode_command] {
            assert!(contrast(t.mode_fg, m) >= 4.5, "{n}: mode badge {m:?} {:.2}", contrast(t.mode_fg, m));
        }
    }
}

/// Workspace colors are the same in every theme (a workspace keeps its color): marks (3:1) on
/// the background of every dark theme, and apart from each other.
#[test]
fn workspace_colors_read_on_every_dark_theme() {
    for t in truecolor().filter(|t| dark(t)) {
        assert_eq!(t.workspaces, &WORKSPACE_COLORS, "{}", t.name);
        for c in t.workspaces {
            assert!(contrast(*c, t.bg) >= 3.0, "{}: {c:?} {:.2}", t.name, contrast(*c, t.bg));
        }
    }
}

/// Each family has a light and a dark variant, picked by the terminal's background.
#[test]
fn a_family_takes_its_variant_by_the_background() {
    for (name, light, dark_t) in FAMILIES {
        assert!(!dark(light) && dark(dark_t), "{name}");
        assert_eq!(resolve(name, true, Background::Light).name, light.name);
        assert_eq!(resolve(name, true, Background::Dark).name, dark_t.name);
        assert_eq!(resolve(name, true, Background::Unknown).name, dark_t.name, "dark when not known");
    }
    for t in BUILTINS {
        assert!(NAMES.contains(&t.name), "{} is a name the setting takes", t.name);
        assert_eq!(resolve(t.name, true, Background::Light).name, t.name, "a theme named is a theme taken");
    }
}

#[test]
fn no_color_means_no_color_anywhere() {
    let t = Theme::no_color();
    for c in colors(&t) {
        assert_eq!(c, Color::Reset);
    }
    for s in [t.text(), t.muted(), t.faint(), t.title(true), t.border(true), t.badge(), t.current(), t.key()] {
        assert_eq!((s.fg.unwrap_or(Color::Reset), s.bg.unwrap_or(Color::Reset)), (Color::Reset, Color::Reset));
    }
    let env = |v: &'static str| move |k: &str| (k == "NO_COLOR").then(|| v.to_string());
    assert_eq!(Theme::from_env("tokyo-night", env("1"), Background::Dark), Theme::no_color());
    assert_eq!(Theme::from_env("auto", env(""), Background::Unknown), Theme::terminal(), "an empty NO_COLOR is unset");
}

#[test]
fn the_terminal_theme_uses_only_the_terminal_palette() {
    for t in [resolve("terminal", false, Background::Dark), resolve("terminal", false, Background::Light)] {
        for c in colors(&t) {
            assert!(!matches!(c, Color::Rgb(..) | Color::Indexed(_)), "{c:?} is not one of the 16 ANSI colors");
        }
    }
    // Muted text is apart from the faint one on a dark background, and readable on a light one.
    assert_eq!(resolve("terminal", false, Background::Dark).fg_muted, Color::Gray);
    assert_eq!(resolve("terminal", false, Background::Light).fg_muted, Color::DarkGray);
}

#[test]
fn auto_takes_tokyo_night_on_a_truecolor_terminal_and_its_variant_by_the_background() {
    let env = |colorterm: &'static str| move |k: &str| (k == "COLORTERM").then(|| colorterm.to_string());
    assert_eq!(Theme::from_env("auto", env("truecolor"), Background::Unknown).name, "tokyo-night-night");
    assert_eq!(Theme::from_env("auto", env("24bit"), Background::Light).name, "tokyo-night-day");
    assert_eq!(Theme::from_env("auto", env(""), Background::Dark).name, "terminal");
    assert_eq!(Theme::from_env("auto", |_| None, Background::Dark).name, "terminal");
    assert_eq!(Theme::from_env("dark", |_| None, Background::Light).name, "dark", "a theme named is a theme taken");
    assert_eq!(Theme::from_env("tokyo-night-day", |_| None, Background::Dark).name, "tokyo-night-day");
    for n in NAMES {
        let _ = resolve(n, true, Background::Unknown);
    }
    assert_eq!(slakio_core::config::THEMES, NAMES, "the config file accepts exactly the theme names");
}

#[test]
fn mode_badges_are_apart_in_color_and_still_marked_without_it() {
    for t in BUILTINS {
        let bgs = [t.mode_normal, t.mode_command, t.mode_insert, t.mode_visual];
        for (i, a) in bgs.iter().enumerate() {
            assert!(!bgs[i + 1..].contains(a), "{}: badge {i}", t.name);
        }
    }
    assert!(Theme::no_color().mode(Color::Reset).add_modifier.contains(Modifier::REVERSED));
}

#[test]
fn workspace_colours_differ_and_wrap_around() {
    for t in [&TERMINAL, &TOKYO_NIGHT_NIGHT] {
        for (i, a) in t.workspaces.iter().enumerate() {
            assert!(!t.workspaces[i + 1..].contains(a), "{}: slot {i}", t.name);
        }
        let n = t.workspaces.len() as u8;
        assert_eq!(t.workspace(WorkspaceColor(n + 1)), t.workspace(WorkspaceColor(1)));
    }
}

/// A row with gray text and a red badge, painted as each kind of selection.
fn painted(t: &Theme, how: Selection) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
    buf.set_string(1, 0, "ab", Style::new().fg(t.fg_dim));
    buf.set_string(4, 0, "3", t.badge());
    t.paint_selection(&mut buf, Rect::new(0, 0, 6, 1), how);
    buf
}

#[test]
fn a_selection_is_a_background_or_a_gutter_bar_never_an_underline() {
    for t in [resolve("terminal", false, Background::Dark), TOKYO_NIGHT_NIGHT, Theme::no_color()] {
        for how in [Selection::Focused, Selection::Unfocused, Selection::Visual] {
            let buf = painted(&t, how);
            for c in buf.content() {
                assert!(!c.modifier.contains(Modifier::UNDERLINED), "{} {how:?}", t.name);
                // Gray on a gray bar would vanish: the text takes the body color.
                assert!(c.bg == Color::Reset || c.fg != c.bg, "{} {how:?}: {:?} on {:?}", t.name, c.fg, c.bg);
            }
            // The badge keeps its own background.
            if t.kind != Kind::NoColor {
                assert_eq!(buf[(4, 0)].bg, t.error, "{} {how:?}", t.name);
            }
        }
    }
    let tokyo = painted(&TOKYO_NIGHT_NIGHT, Selection::Focused);
    assert!((0..6).filter(|&x| x != 4).all(|x| tokyo[(x, 0)].bg == TOKYO_NIGHT_NIGHT.selection), "the whole row");
    let tokyo = painted(&TOKYO_NIGHT_NIGHT, Selection::Unfocused);
    assert_eq!(tokyo[(1, 0)].bg, TOKYO_NIGHT_NIGHT.cursor_line);
    let ansi = painted(&TERMINAL, Selection::Unfocused);
    assert_eq!((ansi[(0, 0)].symbol(), ansi[(1, 0)].bg), (GUTTER, Color::Reset), "a bar, no background");
    let plain = painted(&Theme::no_color(), Selection::Focused);
    assert!(plain[(1, 0)].modifier.contains(Modifier::REVERSED));
}

#[test]
fn dimming_blends_truecolor_and_marks_ansi() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
    buf.set_string(0, 0, "x", Style::new().fg(TOKYO_NIGHT_NIGHT.fg));
    TOKYO_NIGHT_NIGHT.dim_area(&mut buf, Rect::new(0, 0, 2, 1));
    assert_ne!(buf[(0, 0)].fg, TOKYO_NIGHT_NIGHT.fg);
    assert_eq!(buf[(0, 0)].symbol(), "x");
    let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
    TERMINAL.dim_area(&mut buf, Rect::new(0, 0, 2, 1));
    assert!(buf[(1, 0)].modifier.contains(Modifier::DIM));
}

/// The hue of an RGB color in degrees, and its saturation (0 to 1).
fn hue(c: Color) -> (f64, f64) {
    let Color::Rgb(r, g, b) = c else { panic!("{c:?} is not RGB") };
    let (r, g, b) = (f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0);
    }
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, d / max)
}

/// Initials on an avatar chip are readable, the chips differ from each other, none is in the
/// red family (red is for mentions only: the pills), a chip on a dark theme is a quiet tint
/// (never brighter than the selection bar or the accent), and a selection bar keeps them.
#[test]
fn every_avatar_chip_keeps_its_initials_readable_and_quiet() {
    for t in truecolor() {
        let n = t.name;
        assert!(t.avatars.len() >= 6, "{n}");
        let mut bgs = Vec::new();
        for slot in 0..t.avatars.len() {
            let chip = t.avatar(slot);
            let (fg, bg) = (chip.fg.unwrap(), chip.bg.unwrap());
            let min = if dark(t) { 4.3 } else { 4.5 };
            assert!(contrast(fg, bg) >= min, "{n}: initials on chip {slot} {:.2}", contrast(fg, bg));
            assert!(!t.workspaces.contains(&bg), "{n}: chip {slot} is a workspace's color");
            for (k, token) in [("error", t.error), ("accent", t.accent), ("normal", t.mode_normal)] {
                assert_ne!(bg, token, "{n}: chip {slot} is the {k} color");
            }
            // Not red, pink or salmon, and far from the pills' red.
            let (h, sat) = hue(t.avatars[slot]);
            let (e, _) = hue(t.error);
            let apart = (h - e).abs().min(360.0 - (h - e).abs());
            if sat > 0.15 {
                assert!((35.0..=290.0).contains(&h), "{n}: chip {slot} hue {h:.0} is in the red family");
                assert!(apart >= 40.0, "{n}: chip {slot} hue {h:.0} is near the pills' {e:.0}");
            }
            if dark(t) {
                // A tint: no louder than the selection bar, far below the accent.
                assert!(luminance(bg) <= luminance(t.selection), "{n}: chip {slot} brighter than the selection bar");
                assert!(luminance(bg) < luminance(t.accent), "{n}: chip {slot} brighter than the accent");
            }
            bgs.push(bg);
        }
        bgs.sort_by_key(|c| format!("{c:?}"));
        bgs.dedup();
        assert_eq!(bgs.len(), t.avatars.len(), "{n}: chips differ");
        // The group DM chip: a visible chip, not a stray number on the background.
        let group = t.avatar_group();
        let gbg = group.bg.unwrap();
        assert!(contrast(gbg, t.bg) >= 1.15, "{n}: group chip on the background {:.2}", contrast(gbg, t.bg));
        assert!(contrast(group.fg.unwrap(), gbg) >= 4.5, "{n}: group chip text");
    }
    // The 16-color theme: bright colors under black, no red; the group chip has a background.
    assert!(TERMINAL.avatars.iter().all(|c| !matches!(c, Color::Black | Color::DarkGray | Color::Reset | Color::Red)));
    assert!(!matches!(TERMINAL.avatar_group().bg, None | Some(Color::Reset)));
    for t in BUILTINS {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        buf[(1, 0)].set_style(t.avatar(3));
        buf[(2, 0)].set_style(t.avatar_group());
        t.paint_selection(&mut buf, Rect::new(0, 0, 4, 1), Selection::Focused);
        if t.kind != Kind::NoColor {
            assert_eq!(Some(buf[(1, 0)].bg), t.avatar(3).bg, "{}: the chip keeps its color", t.name);
            assert_eq!(Some(buf[(2, 0)].bg), t.avatar_group().bg, "{}: the group chip too", t.name);
        }
    }
    let plain = Theme::no_color().avatar(0);
    assert!(plain.add_modifier.contains(Modifier::REVERSED | Modifier::BOLD), "no colors: reversed");
}
