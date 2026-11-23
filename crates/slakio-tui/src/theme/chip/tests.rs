use super::super::{BUILTINS, Kind, TERMINAL, Theme};
use super::{INITIALS, MUTED};
use ratatui::style::Color;
use std::borrow::Cow;

/// WCAG 2 relative luminance, worked out here apart from the code under test.
fn luminance(c: Color) -> f64 {
    let Color::Rgb(r, g, b) = c else { panic!("{c:?} is not RGB") };
    let lin = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.039_28 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

fn ratio(a: Color, b: Color) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Every built-in, with the terminal's own colours also on a light background.
fn themes() -> Vec<Theme> {
    let mut v: Vec<Theme> = BUILTINS.iter().map(|t| (*t).clone()).collect();
    v.push(super::super::resolve("terminal", false, super::super::Background::Light));
    v
}

fn tinted(t: &Theme, tint: u16) -> Theme {
    Theme { avatar_tint: tint, ..t.clone() }
}

/// Where both colours are RGB, the measured WCAG ratio of a chip's text on its background.
fn measured(t: &Theme, what: &str, fg: Option<Color>, bg: Option<Color>, min: f64, worst: &mut f64) {
    let (fg, bg) = (fg.unwrap(), bg.unwrap());
    let k = ratio(fg, bg);
    *worst = worst.min(k);
    assert!(k >= min, "{} tint {}: {what} {fg:?} on {bg:?} is {k:.2}:1", t.name, t.avatar_tint);
}

/// Initials stay 4.5:1 on their chip (muted ones 3:1, a group's count 4.5:1) at every tint the
/// format takes below 100, on every built-in that draws RGB chips; the closer the tint is to
/// the hue, the more the initials step away from it.
#[test]
fn chip_text_keeps_its_contrast_at_every_tint_on_every_builtin() {
    let mut worst = f64::MAX;
    let mut truecolor = 0;
    for base in themes().iter().filter(|t| t.kind == Kind::Truecolor) {
        truecolor += 1;
        for tint in 0..=99 {
            let t = tinted(base, tint);
            for slot in 0..t.avatars.len() {
                let s = t.avatar(slot);
                measured(&t, &format!("initials of chip {slot}"), s.fg, s.bg, INITIALS, &mut worst);
            }
            let g = t.avatar_group();
            measured(&t, "group count", g.fg, g.bg, INITIALS, &mut worst);
            let m = t.avatar_muted();
            measured(&t, "muted initials", m.fg, m.bg, MUTED, &mut worst);
        }
    }
    assert!(truecolor >= 11, "every built-in but the terminal's own colours");
    assert!(worst >= MUTED, "worst {worst:.2}");
}

/// The terminal's own colours (a 16-colour theme, light or dark): the chip has no background
/// to measure against, so the initials are the hue on the terminal's background at every tint,
/// never a colour made up for them.
#[test]
fn terminal_colour_chips_keep_their_hue_at_every_tint() {
    for base in themes().iter().filter(|t| t.kind == Kind::Ansi) {
        for tint in 0..=99 {
            let t = tinted(base, tint);
            for slot in 0..t.avatars.len() {
                let s = t.avatar(slot);
                assert_eq!(s.bg, None, "{} tint {tint} chip {slot}", t.name);
                assert_eq!(s.fg, Some(t.avatars[slot]), "{} tint {tint} chip {slot}", t.name);
                assert!(!matches!(s.fg, Some(Color::Rgb(..))));
            }
            assert_eq!(t.avatar_group().bg, None);
            assert_eq!(t.avatar_muted().bg, None);
        }
    }
}

/// A theme file on the terminal's colours with `#rrggbb` avatars draws 24-bit chips on the
/// terminal's background: the hue is solid there under `mode_fg`, a terminal colour that
/// cannot be measured, so the initials take black or white, 4.5:1 on every hue, light, dark or
/// mid gray. A chip whose background is a terminal colour keeps its text as it is.
#[test]
fn rgb_chips_under_terminal_colour_text_get_measurable_initials() {
    const HUES: [Color; 6] = [
        Color::Rgb(0x77, 0x77, 0x77),
        Color::Rgb(0xFF, 0xFF, 0x00),
        Color::Rgb(0x00, 0x00, 0x80),
        Color::Rgb(0x11, 0x11, 0x11),
        Color::Rgb(0xF0, 0xF0, 0xF0),
        Color::Rgb(0x2E, 0x8B, 0x57),
    ];
    for mode_fg in [Color::Black, Color::Reset, Color::White] {
        let base = Theme { kind: Kind::Truecolor, avatars: Cow::Borrowed(&HUES), mode_fg, ..TERMINAL };
        for tint in 0..=99 {
            let t = tinted(&base, tint);
            for (slot, &hue) in HUES.iter().enumerate() {
                let s = t.avatar(slot);
                assert_eq!(s.bg, Some(hue), "solid on the terminal's background");
                let fg = s.fg.unwrap();
                assert!(matches!(fg, Color::Rgb(..)), "{mode_fg:?}: {fg:?}");
                assert!(ratio(fg, hue) >= INITIALS, "{mode_fg:?} on {hue:?}: {:.2}", ratio(fg, hue));
            }
            // The group chip's background is the terminal's own (no RGB to tint): left alone.
            let g = t.avatar_group();
            assert_eq!(g.bg, Some(Color::Reset));
            assert_eq!(g.fg, Some(TERMINAL.fg));
        }
    }
}

/// The step toward the pole is the least that does: a colour already readable stays, a colour
/// on itself ends at least 4.5:1 away, and a terminal background is not measured.
#[test]
fn readable_moves_only_as_far_as_it_has_to() {
    use super::readable;
    let (navy, white) = (Color::Rgb(0x00, 0x00, 0x80), Color::Rgb(0xFF, 0xFF, 0xFF));
    assert_eq!(readable(white, navy, INITIALS), white);
    let teal = Color::Rgb(0x2E, 0x8B, 0x57);
    let fg = readable(teal, teal, INITIALS);
    assert!(ratio(fg, teal) >= INITIALS && fg != teal);
    assert_eq!(readable(Color::Cyan, Color::Reset, INITIALS), Color::Cyan);
    assert_eq!(readable(teal, Color::Blue, INITIALS), teal);
}

/// The workspace badge on the list's title keeps 4.5:1 on the background in every workspace
/// color of every built-in that draws RGB.
#[test]
fn the_workspace_badge_keeps_its_contrast_on_every_builtin() {
    let mut worst = f64::MAX;
    for t in themes().iter().filter(|t| t.kind == Kind::Truecolor) {
        for slot in 0..t.workspaces.len() {
            let c = slakio_core::model::WorkspaceColor(u8::try_from(slot).unwrap());
            measured(t, &format!("workspace badge {slot}"), t.workspace_text(c).fg, Some(t.bg), INITIALS, &mut worst);
        }
    }
    assert!(worst < 99.0, "measured");
}
