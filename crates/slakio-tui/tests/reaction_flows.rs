//! Reactions drawn as Slack's desktop app draws them: quiet pills a little apart from the
//! background, the emoji then the count, one cell apart. Icons on, each end is a rounded cap in
//! the chip's colour on the row's background; icons off, a blank pad on the chip's colour, in the
//! same cell. The user's own sits on a light wash of the accent with its count in the accent; no
//! cell is ever filled with the accent. A selected message keeps its chips as they are.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use slakio_core::i18n::Lang;
use slakio_tui::app::{PaneKind, Settings};
use slakio_tui::theme::{Background, Theme, resolve};
use slakio_tui::ui::timeline::chips::{CAP_LEFT, CAP_RIGHT};

const SIZES: [(u16, u16); 2] = [(80, 24), (120, 40)];

fn themes() -> [Theme; 2] {
    [resolve("dark", true, Background::Dark), resolve("light", true, Background::Light)]
}

/// The demo in `theme` at `w`×`h`, icons `icons`, `general` open with its newest message with
/// the user's own reaction selected (so on screen).
fn open(theme: &Theme, (w, h): (u16, u16), icons: bool) -> Demo {
    let mut d = Demo::with(w, h, Lang::En, Settings { icons, ..Settings::default() });
    d.app.theme = theme.clone();
    d.open("general");
    let msgs = d.pane(PaneKind::Conversation).unwrap().messages().to_vec();
    let i = (0..msgs.len()).rev().find(|&i| msgs[i].reactions.iter().any(|r| r.mine)).expect("an own reaction");
    d.app.select_message(i);
    d
}

/// [`open`], then the selection moved to a message next to it, so its chips show unselected.
fn beside(theme: &Theme, size: (u16, u16), icons: bool) -> Demo {
    let mut d = open(theme, size, icons);
    let i = d.pane(PaneKind::Conversation).unwrap().selected().expect("a selected message");
    let n = d.pane(PaneKind::Conversation).unwrap().messages().len();
    for j in [i + 1, i.wrapping_sub(1)].into_iter().filter(|&j| j < n) {
        d.app.select_message(j);
        if chips(&d, &d.buffer()).iter().any(|c| c.message == i) {
            return d;
        }
    }
    panic!("{} {size:?}: no neighbour leaves the chips on screen", theme.name)
}

/// A chip drawn: its message, whether it is the user's own, its count, the cells of its left
/// end, its emoji and its right end, and the cells of its count.
#[derive(Debug, PartialEq, Eq)]
struct Chip {
    message: usize,
    mine: bool,
    y: u16,
    left: u16,
    emoji: u16,
    count: Vec<u16>,
    right: u16,
}

/// Every chip of a standard emoji drawn in the conversation.
fn chips(d: &Demo, buf: &Buffer) -> Vec<Chip> {
    let pane = d.pane(PaneKind::Conversation).unwrap();
    let mut out = Vec::new();
    for (i, y) in pane.drawn_rows() {
        for r in &pane.messages()[i].reactions {
            let Some(e) = slakio_tui::emoji::get(r.name.as_str()) else { continue };
            let count = format!(" {}", r.count);
            let wide = slakio_tui::text::width(e) as u16;
            let found = (1..buf.area.width.saturating_sub(wide + count.len() as u16 + 1)).find(|&x| {
                buf[(x, y)].symbol().starts_with(e)
                    && (0..count.len() as u16)
                        .all(|k| buf[(x + wide + k, y)].symbol() == &count[usize::from(k)..usize::from(k) + 1])
            });
            if let Some(x) = found {
                let digits: Vec<u16> = (x + wide..x + wide + count.len() as u16).collect();
                let right = x + wide + count.len() as u16;
                out.push(Chip { message: i, mine: r.mine, y, left: x - 1, emoji: x, count: digits, right });
            }
        }
    }
    out
}

/// The background of the user's own chip or another's.
fn chip_bg(t: &Theme, mine: bool) -> Color {
    if mine { t.reaction_mine } else { t.reaction }
}

#[test]
fn mine_and_others_chips_have_their_exact_cells() {
    let (mut mine_seen, mut others_seen) = (0, 0);
    for t in themes() {
        for size in SIZES {
            for (icons, selected) in [(true, true), (true, false), (false, true), (false, false)] {
                let d = if selected { open(&t, size, icons) } else { beside(&t, size, icons) };
                let buf = d.buffer();
                let what = format!("{} {size:?} icons {icons} selected {selected}", t.name);
                let drawn = chips(&d, &buf);
                assert!(!drawn.is_empty(), "{what}: chips drawn\n{}", d.screen());
                for c in &drawn {
                    let bg = chip_bg(&t, c.mine);
                    let e = &buf[(c.emoji, c.y)];
                    assert_eq!((e.fg, e.bg), (t.fg, bg), "{what}: the emoji of {c:?}");
                    let count_fg = if c.mine { t.accent } else { t.fg_muted };
                    for &x in &c.count {
                        let cell = &buf[(x, c.y)];
                        assert_eq!(cell.bg, bg, "{what}: count cell {x} of {c:?}");
                        if cell.symbol() != " " {
                            assert_eq!(cell.fg, count_fg, "{what}: count cell {x} of {c:?}");
                            assert_eq!(cell.modifier.contains(Modifier::BOLD), c.mine, "{what}: {c:?}");
                        }
                    }
                    // The row's own background, where the cell before the chip shows it.
                    let row = buf[(c.left - 1, c.y)].bg;
                    for (x, cap) in [(c.left, CAP_LEFT), (c.right, CAP_RIGHT)] {
                        let cell = &buf[(x, c.y)];
                        if icons {
                            assert_eq!((cell.symbol(), cell.fg, cell.bg), (cap, bg, row), "{what}: end {x} of {c:?}");
                        } else {
                            assert_eq!((cell.symbol(), cell.bg), (" ", bg), "{what}: pad {x} of {c:?}");
                        }
                    }
                    if c.mine { mine_seen += 1 } else { others_seen += 1 }
                }
            }
        }
    }
    assert!(mine_seen > 0 && others_seen > 0, "{mine_seen} own chips, {others_seen} others");
}

#[test]
fn no_cell_is_filled_with_the_accent() {
    for t in themes() {
        for size in SIZES {
            for icons in [true, false] {
                let mut d = open(&t, size, icons);
                let pane = d.pane(PaneKind::Conversation).unwrap();
                let area = d.app.message_area(pane.handle()).unwrap();
                let i = (0..pane.messages().len()).rev().find(|&i| pane.messages()[i].reactions.iter().any(|r| r.mine));
                let mut frames = vec![d.buffer()];
                if let Some(i) = i {
                    d.app.select_message(i);
                    frames.push(d.buffer());
                }
                for buf in frames {
                    for y in area.top()..area.bottom() {
                        for x in area.left()..area.right() {
                            assert_ne!(buf[(x, y)].bg, t.accent, "{} {size:?} {icons}: cell {x},{y}", t.name);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn icons_off_has_no_caps_and_padded_chips() {
    for t in themes() {
        for size in SIZES {
            let on = beside(&t, size, true);
            let off = beside(&t, size, false);
            let (on_buf, off_buf) = (on.buffer(), off.buffer());
            let caps =
                |buf: &Buffer| buf.content().iter().filter(|c| [CAP_LEFT, CAP_RIGHT].contains(&c.symbol())).count();
            assert_eq!(caps(&off_buf), 0, "{} {size:?}: no cap without icons", t.name);
            let (on_chips, off_chips) = (chips(&on, &on_buf), chips(&off, &off_buf));
            assert!(!off_chips.is_empty(), "{} {size:?}", t.name);
            assert_eq!(caps(&on_buf), 2 * on_chips.len(), "{} {size:?}: two caps a chip", t.name);
            // A cap takes the pad's cell: the chips wrap, line up and sit where they did.
            assert_eq!(on_chips, off_chips, "{} {size:?}", t.name);
            for c in &off_chips {
                let bg = chip_bg(&t, c.mine);
                for x in [c.left, c.right] {
                    assert_eq!((off_buf[(x, c.y)].symbol(), off_buf[(x, c.y)].bg), (" ", bg), "{} {c:?}", t.name);
                }
                // One cell apart from the next chip on its row, on the row's background.
                if let Some(next) = off_chips.iter().find(|n| n.y == c.y && n.left > c.right) {
                    assert_eq!(next.left, c.right + 2, "{} {c:?}", t.name);
                    assert!(![t.reaction, t.reaction_mine].contains(&off_buf[(c.right + 1, c.y)].bg));
                }
            }
        }
    }
}

#[test]
fn a_selected_message_keeps_its_chips() {
    for t in themes() {
        for size in SIZES {
            for icons in [true, false] {
                let mut d = beside(&t, size, icons);
                let shown = chips(&d, &d.buffer());
                let Some(i) = shown.last().map(|c| c.message) else { panic!("{} {size:?}: chips drawn", t.name) };
                d.app.select_message(i);
                let buf = d.buffer();
                let what = format!("{} {size:?} icons {icons}", t.name);
                let selected: Vec<Chip> = chips(&d, &buf).into_iter().filter(|c| c.message == i).collect();
                assert!(!selected.is_empty(), "{what}");
                for c in &selected {
                    let bg = chip_bg(&t, c.mine);
                    let cells = std::iter::once(c.emoji).chain(c.count.iter().copied());
                    for x in cells {
                        assert_eq!(buf[(x, c.y)].bg, bg, "{what}: cell {x} of {c:?} keeps its chip");
                    }
                    assert_eq!(buf[(c.left - 1, c.y)].bg, t.selection, "{what}: the bar around {c:?}");
                    for x in [c.left, c.right] {
                        let cell = &buf[(x, c.y)];
                        let want = if icons { (bg, t.selection) } else { (cell.fg, bg) };
                        assert_eq!((cell.fg, cell.bg), want, "{what}: end {x} of {c:?}");
                    }
                }
            }
        }
    }
}
