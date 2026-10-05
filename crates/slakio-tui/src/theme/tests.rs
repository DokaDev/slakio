use super::*;
use slakio_core::model::WorkspaceColor;

fn styles(t: &Theme) -> Vec<Style> {
    let mut v = vec![
        t.text,
        t.muted,
        t.title,
        t.status,
        t.mode_normal,
        t.mode_command,
        t.warning,
        t.border_focus,
        t.border,
        t.cursor,
        t.cursor_inactive,
        t.section,
        t.unread,
        t.mention,
        t.muted_conversation,
        t.current,
        t.connection,
        t.mode_insert,
        t.mode_visual,
        t.author,
        t.own_author,
        t.timestamp,
        t.reaction,
        t.reaction_mine,
        t.thread_link,
    ];
    v.extend(t.workspaces);
    v
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
    let bgs = [t.mode_normal.bg, t.mode_command.bg, t.mode_insert.bg, t.mode_visual.bg];
    for (i, a) in bgs.iter().enumerate() {
        assert!(!bgs[i + 1..].contains(a), "badge {i}");
    }
    assert!(Theme::no_color().mode_normal.add_modifier.contains(Modifier::REVERSED));
}

#[test]
fn workspace_colours_differ_and_wrap_around() {
    let t = Theme::terminal();
    for (i, a) in t.workspaces.iter().enumerate() {
        assert!(!t.workspaces[i + 1..].contains(a), "slot {i}");
    }
    assert_eq!(t.workspace(WorkspaceColor(5)), t.workspace(WorkspaceColor(1)));
    // Without colour the cursor still shows.
    assert!(Theme::no_color().cursor.add_modifier.contains(Modifier::REVERSED));
}
