use super::*;

fn styles(t: &Theme) -> [Style; 7] {
    [t.text, t.muted, t.title, t.status, t.mode_normal, t.mode_command, t.warning]
}

#[test]
fn no_color_means_no_color_anywhere() {
    for s in styles(&Theme::no_color()) {
        assert_eq!((s.fg, s.bg), (None, None), "{s:?}");
    }
    let env = |v: &'static str| move |k: &str| (k == "NO_COLOR").then(|| v.to_string());
    assert_eq!(Theme::from_env(env("1")), Theme::no_color());
    assert_eq!(Theme::from_env(env("")), Theme::terminal(), "an empty NO_COLOR is unset");
    assert_eq!(Theme::from_env(|_| None), Theme::terminal());
}

#[test]
fn the_default_theme_uses_only_the_terminal_palette() {
    for s in styles(&Theme::terminal()) {
        for c in [s.fg, s.bg].into_iter().flatten() {
            assert!(!matches!(c, Color::Rgb(..) | Color::Indexed(_)), "{c:?} is not one of the 16 ANSI colors");
        }
    }
}

#[test]
fn mode_badges_are_apart_in_color_and_still_marked_without_it() {
    // Each badge also carries its mode name (the status line draws it).
    let t = Theme::terminal();
    assert_ne!(t.mode_normal.bg, t.mode_command.bg);
    assert!(Theme::no_color().mode_normal.add_modifier.contains(Modifier::REVERSED));
}
