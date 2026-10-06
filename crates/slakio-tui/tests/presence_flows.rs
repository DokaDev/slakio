//! A DM shows its peer's presence before the name, in the list and in the conversation's title:
//! `●` active, `○` away, `◐` do not disturb. The shape tells them apart, so they read without
//! color; with color, active is the success color and away is muted.

#[path = "support/demo.rs"]
mod demo;

use demo::{Demo, mask_hangul};
use ratatui::style::Color;
use slakio_core::i18n::Lang;
use slakio_core::model::{ConversationKind, Presence};
use slakio_tui::app::Settings;
use slakio_tui::app::model::Row;
use slakio_tui::app::shell::View;
use slakio_tui::screen;
use slakio_tui::theme::{Background, Theme, resolve};

/// The demo on DMs, in `theme`.
fn dms(theme: Theme, settings: Settings) -> Demo {
    let mut d = Demo::with(120, 40, Lang::En, settings);
    d.app.theme = theme;
    d.keys("space d");
    assert_eq!(d.app.shell.view, View::Dms);
    d
}

/// The presence of the peer of the DM named `name` (in the list shown).
fn presence_of(d: &Demo, name: &str) -> Presence {
    let model = &d.app.model;
    let Row::Conversation(i) = d.app.shell.rows(model)[index_of(d, name)] else { unreachable!() };
    let ConversationKind::Dm { user } = &model.conversation(i).kind else { panic!("{name} is not a DM") };
    model.user(user).expect("the peer").presence
}

/// The index of the DM named `name` among the list's rows.
fn index_of(d: &Demo, name: &str) -> usize {
    let rows = d.app.shell.rows(&d.app.model);
    (0..rows.len())
        .find(|&i| matches!(rows[i], Row::Conversation(c) if d.app.model.conversation(c).name == name))
        .unwrap_or_else(|| panic!("no row {name}"))
}

/// Where the mark of the DM named `name` is drawn: (x, y). DMs has no sections, so the mark is
/// the first cell after the gutter, or the cell after the avatar chip there.
fn mark_at(d: &Demo, name: &str) -> (u16, u16) {
    let list = screen::inner(d.app.areas().list.expect("the list panel"));
    let y = list.y + (index_of(d, name) - d.app.shell.list_top) as u16;
    (list.x + if d.app.settings.avatars { 3 } else { 1 }, y)
}

fn muted(d: &Demo, name: &str) -> bool {
    let Row::Conversation(i) = d.app.shell.rows(&d.app.model)[index_of(d, name)] else { unreachable!() };
    d.app.model.conversation(i).muted
}

/// The first DMs: Latin and Korean names (escapes: no Hangul in source files).
const PEOPLE: [&str; 6] =
    ["Minsu Kim", "Jiho Park", "\u{C774}\u{C11C}\u{C5F0}", "Jiyoung Lee", "Alex Morgan", "\u{BC15}\u{C900}\u{D638}"];

#[test]
fn every_dm_shows_its_peers_presence_by_shape() {
    let d = dms(Theme::no_color(), Settings::default());
    let buf = d.buffer();
    let mut seen = Vec::new();
    for name in PEOPLE {
        let mark = buf[mark_at(&d, name)].symbol().to_string();
        let want = match presence_of(&d, name) {
            Presence::Active => "●",
            Presence::Away => "○",
            Presence::Dnd => "◐",
            Presence::Unknown => "@",
        };
        assert_eq!(mark, want, "{name}");
        seen.push(want);
    }
    for m in ["●", "○", "◐"] {
        assert!(seen.contains(&m), "the demo shows {m}: {seen:?}");
    }
    insta::assert_snapshot!("dms_presence_120x40", mask_hangul(&d.screen()));
}

#[test]
fn presence_has_a_color_of_its_own_in_a_theme_with_colors() {
    for theme in [resolve("terminal", false, Background::Dark), resolve("tokyo-night", true, Background::Dark)] {
        let d = dms(theme.clone(), Settings::default());
        let buf = d.buffer();
        for name in PEOPLE {
            let fg = buf[mark_at(&d, name)].fg;
            let want = match presence_of(&d, name) {
                // A muted DM is faint, its mark too.
                _ if muted(&d, name) => theme.fg_dim,
                Presence::Active => theme.success,
                Presence::Dnd => theme.warning,
                _ => theme.fg_muted,
            };
            // A selected row lifts muted text to the body color.
            assert!(fg == want || fg == theme.fg && want == theme.fg_muted, "{} {name}: {fg:?}", theme.name);
            assert_ne!(fg, Color::Reset, "{} {name}", theme.name);
        }
    }
}

#[test]
fn icons_on_draw_do_not_disturb_as_a_moon() {
    let d = dms(Theme::no_color(), Settings { icons: true, ..Settings::default() });
    let name = PEOPLE.into_iter().find(|n| presence_of(&d, n) == Presence::Dnd).expect("someone in do not disturb");
    assert_eq!(d.buffer()[mark_at(&d, name)].symbol(), "\u{F0594}");
}

#[test]
fn a_dm_title_says_whether_its_peer_is_around() {
    let mut d = dms(Theme::no_color(), Settings::default());
    let name = PEOPLE.into_iter().find(|n| presence_of(&d, n) == Presence::Away).expect("someone away");
    d.app.shell.list_cursor = index_of(&d, name);
    d.keys("enter");
    let title = d.screen().lines().next().unwrap().to_string();
    assert!(title.contains(&format!("@{name} ○ away")), "{title}");
}

#[test]
fn the_selected_row_keeps_its_presence_color_in_the_16_color_theme() {
    let theme = resolve("terminal", false, Background::Dark);
    let mut d = dms(theme.clone(), Settings::default());
    for p in [Presence::Active, Presence::Away, Presence::Dnd] {
        let name = PEOPLE.into_iter().find(|n| presence_of(&d, n) == p && !muted(&d, n)).expect("a peer");
        d.app.shell.list_cursor = index_of(&d, name);
        let buf = d.buffer();
        let cell = &buf[mark_at(&d, name)];
        assert_eq!(cell.bg, theme.selection, "{name} is on the selection bar");
        let want = match p {
            Presence::Active => theme.success,
            Presence::Dnd => theme.warning,
            _ => theme.fg_muted,
        };
        assert_eq!(cell.fg, want, "{name} ({p:?}) keeps its color");
    }
}
