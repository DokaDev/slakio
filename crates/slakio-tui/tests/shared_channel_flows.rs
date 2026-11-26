//! A channel shared with another organization (Slack Connect) says so by its prefix: `⌗` with
//! icons off, the shared glyph with icons on; no link at its end (only a DM keeps that). A private
//! shared channel keeps its lock. Its pane is titled `⌗partner-shared`, a plain channel's
//! `#backend`. At 80x24 and 120x40, icons off and on.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use demo::Demo;
use ratatui::buffer::Buffer;
use scripted::Scripted;
use slakio_core::i18n::Lang;
use slakio_core::model::ConversationKind;
use slakio_tui::app::Settings;
use slakio_world::World;
use slakio_world::names::SHARED;

const ICON_SHARED: &str = "\u{F0497}";
const ICON_PRIVATE: &str = "\u{F033E}";
const ICON_LINK: char = '\u{F0339}';
const SIZES: [(u16, u16); 2] = [(80, 24), (120, 40)];

/// The list panel's row of `name` (left of the panel's right border).
fn row<'a>(screen: &'a str, name: &str) -> &'a str {
    let line = screen
        .lines()
        .find(|l| l.split("││").next().is_some_and(|left| left.contains(&format!(" {name}"))))
        .unwrap_or_else(|| panic!("{name}: {screen}"));
    line.split("││").next().unwrap()
}

/// The cell of `buf` on row `y` whose symbol starts with `glyph`: drawn in its two-cell slot, the
/// cell after it blank.
fn glyph_slot(buf: &Buffer, y: u16, glyph: &str) -> Option<u16> {
    let x = (0..buf.area.width).find(|&x| buf[(x, y)].symbol().starts_with(glyph))?;
    assert_eq!(buf[(x, y)].symbol(), format!("{glyph} "), "the glyph and the blank of its slot");
    assert_eq!(buf[(x + 1, y)].symbol(), " ");
    Some(x)
}

fn demo(w: u16, h: u16, icons: bool) -> Demo {
    Demo::with(w, h, Lang::En, Settings { icons, ..Settings::default() })
}

fn line_of(screen: &str, needle: &str) -> u16 {
    screen.lines().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle}: {screen}")) as u16
}

#[test]
fn a_slack_connect_channel_shows_the_shared_prefix_and_no_link() {
    for ((w, h), icons) in SIZES.into_iter().flat_map(|s| [(s, false), (s, true)]) {
        let d = demo(w, h, icons);
        let screen = d.screen();
        let at = format!("{w}x{h} icons {icons}: {screen}");
        let shared = row(&screen, SHARED);
        if icons {
            assert!(shared.contains(&format!("{ICON_SHARED} {SHARED}")), "{at}");
            let y = line_of(&screen, &format!("{ICON_SHARED} {SHARED}"));
            assert!(glyph_slot(&d.buffer(), y, ICON_SHARED).is_some(), "{at}");
        } else {
            assert!(shared.contains(&format!("\u{2317} {SHARED}")), "{at}");
        }
        assert!(!shared.contains(ICON_LINK) && !shared.contains(" ext"), "no link on a channel: {at}");
        assert!(!shared.contains(&format!("# {SHARED}")), "{at}");
        // A channel not shared keeps `#`.
        assert!(row(&screen, "backend").trim_start_matches('│').trim_start().starts_with("# backend"), "{at}");
    }
}

#[test]
fn a_private_shared_channel_keeps_its_lock() {
    let mut snapshot = World::demo().snapshot().clone();
    let c = snapshot.conversations.iter_mut().find(|c| c.name == SHARED).unwrap();
    assert!(c.external);
    c.kind = ConversationKind::Channel { private: true };
    for ((w, h), icons) in SIZES.into_iter().flat_map(|s| [(s, false), (s, true)]) {
        let mut s = Scripted::booted(true, Some(snapshot.clone()));
        s.d.app.resize(w, h);
        s.d.app.settings.icons = icons;
        let screen = s.d.screen();
        let at = format!("{w}x{h} icons {icons}: {screen}");
        let shared = row(&screen, SHARED);
        if icons {
            assert!(shared.contains(&format!("{ICON_PRIVATE} {SHARED}")), "{at}");
            let y = line_of(&screen, &format!("{ICON_PRIVATE} {SHARED}"));
            assert!(glyph_slot(&s.d.buffer(), y, ICON_PRIVATE).is_some(), "{at}");
        } else {
            assert!(shared.contains(&format!("\u{25AA} {SHARED}")), "{at}");
        }
        assert!(!shared.contains(ICON_SHARED) && !shared.contains('\u{2317}'), "the lock wins: {at}");
        assert!(!shared.contains(ICON_LINK) && !shared.contains(" ext"), "no link on a channel: {at}");
    }
}

#[test]
fn a_shared_channel_s_pane_is_titled_with_its_mark_a_plain_one_with_a_hash() {
    for ((w, h), icons) in SIZES.into_iter().flat_map(|s| [(s, false), (s, true)]) {
        let mut d = demo(w, h, icons);
        d.open(SHARED);
        let screen = d.screen();
        let at = format!("{w}x{h} icons {icons}: {screen}");
        let title = if icons { format!("▌{ICON_SHARED} {SHARED} ") } else { format!("▌\u{2317}{SHARED} ") };
        assert!(screen.lines().next().unwrap().contains(&title), "{at}");
        if icons {
            let x = glyph_slot(&d.buffer(), 0, ICON_SHARED).expect("the glyph in the title");
            assert!(x > d.app.size.width / 4, "in the pane's title, not the list: {at}");
        }
        assert!(!screen.lines().next().unwrap().contains(&format!("#{SHARED}")), "{at}");
        // The divider over the composer and the status line keep the breadcrumb.
        assert!(screen.contains(&format!("#{SHARED}")), "{at}");
        d.open("backend");
        let screen = d.screen();
        assert!(screen.lines().next().unwrap().contains("▌#backend "), "{w}x{h} icons {icons}: {screen}");
    }
}
