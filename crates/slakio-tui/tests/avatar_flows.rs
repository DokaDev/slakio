//! Initials avatars: a two-cell chip of a person's initials on their color before the sender's
//! name on the first message of a group, before a DM in the list (its presence mark at the
//! chip's corner), and in a DM's title. `:avatars` turns them off and on and asks for the
//! setting to be saved; the text column stays where it is on every row.

#[path = "support/demo.rs"]
mod demo;

use demo::{Demo, mask_hangul};
use ratatui::buffer::Buffer;
use ratatui::style::Modifier;
use slakio_core::i18n::Lang;
use slakio_core::model::{ConversationKind, UserId};
use slakio_tui::app::model::Row;
use slakio_tui::app::shell::View;
use slakio_tui::app::{Effect, Mode, Settings};
use slakio_tui::avatar;
use slakio_tui::screen;
use slakio_tui::theme::{Background, Theme, resolve};

fn tokyo() -> Theme {
    resolve("tokyo-night", true, Background::Dark)
}

/// `c` is the background of a person's chip in `theme`.
fn is_chip(theme: &Theme, c: ratatui::style::Color) -> bool {
    (0..theme.avatars.len()).any(|i| theme.avatar(i).bg == Some(c))
}

/// The text of cells `x..x + w` of row `y` (a wide character's second cell adds nothing).
fn text(buf: &Buffer, x: u16, y: u16, w: u16) -> String {
    let mut out = String::new();
    let mut skip = false;
    for x in x..x + w {
        let s = buf[(x, y)].symbol();
        if !std::mem::take(&mut skip) {
            out.push_str(s);
            skip = slakio_tui::text::width(s) == 2;
        }
    }
    out
}

/// The demo with `#backend` open in `theme`.
fn backend(w: u16, h: u16, theme: Theme, avatars: bool) -> Demo {
    let mut d = Demo::with(w, h, Lang::En, Settings { avatars, ..Settings::default() });
    d.app.theme = theme;
    d.open("backend");
    d
}

/// The first row drawn of each message of the main pane on screen: (message, y).
fn first_rows(d: &Demo) -> Vec<(usize, u16)> {
    let _ = d.buffer();
    let pane = d.app.work.main.as_ref().unwrap();
    let mut out: Vec<(usize, u16)> = Vec::new();
    for h in pane.hits.borrow().iter() {
        if !out.iter().any(|(m, _)| *m == h.message) {
            out.push((h.message, h.y));
        }
    }
    out
}

#[test]
fn a_message_group_starts_with_its_senders_chip_and_the_text_column_stays_put() {
    let theme = tokyo();
    let d = backend(200, 50, theme.clone(), true);
    let buf = d.buffer();
    let pane = d.app.work.main.as_ref().unwrap();
    let (main, _) = d.app.panes();
    // The pane's text starts one cell in (the gutter); the chip is first.
    let x0 = screen::inner(main.unwrap()).x + 1;
    let mut chips = 0;
    for (i, y) in first_rows(&d) {
        let m = &pane.items[i];
        let starts_group = i == 0 || pane.items[i - 1].user != m.user;
        let chip = &buf[(x0, y)];
        let has_chip = is_chip(&theme, chip.bg);
        if starts_group {
            assert!(has_chip, "message {i} by {} starts a group: a chip", m.author);
        }
        if has_chip {
            chips += 1;
            let want = avatar::initials(m.author.as_str(), String::new);
            assert_eq!(text(&buf, x0, y, 2), want, "message {i}");
            let slot = avatar::slot(&m.user) % theme.avatars.len();
            // A wide initial's second cell is the terminal's to fill (with the first's colors).
            let cells = if slakio_tui::text::width(&want) == 2 && want.chars().count() == 1 { 1 } else { 2 };
            for x in (x0..).take(cells) {
                let c = &buf[(x, y)];
                let want = theme.avatar(slot);
                assert_eq!((Some(c.fg), Some(c.bg)), (want.fg, want.bg), "message {i}");
                assert!(c.modifier.contains(Modifier::BOLD));
            }
            assert_eq!(buf[(x0 + 2, y)].bg, theme.bg, "one plain space after the chip");
            assert!(text(&buf, x0 + 3, y, 14).starts_with(&m.author.as_str().chars().take(4).collect::<String>()));
        } else {
            assert_eq!(text(&buf, x0, y, 3).trim(), "", "a grouped message leaves the chip out");
        }
        // The text column: 14 cells of name and 3 of chip, on every row.
        assert_eq!(buf[(x0 + 16, y)].symbol(), " ", "message {i}");
        assert_ne!(buf[(x0 + 17, y)].symbol(), " ", "message {i}: its text starts at the column");
    }
    assert!(chips >= 5, "{chips}");
}

#[test]
fn with_avatars_off_the_name_starts_the_row_as_before() {
    let theme = tokyo();
    let d = backend(200, 50, theme.clone(), false);
    let buf = d.buffer();
    let (main, _) = d.app.panes();
    let x0 = screen::inner(main.unwrap()).x + 1;
    let pane = d.app.work.main.as_ref().unwrap();
    for (i, y) in first_rows(&d) {
        assert!(!is_chip(&theme, buf[(x0, y)].bg), "message {i}");
        if i == 0 || pane.items[i - 1].user != pane.items[i].user {
            let first = pane.items[i].author.as_str().chars().next();
            assert_eq!(text(&buf, x0, y, 3).chars().next(), first, "message {i}");
        }
        assert_ne!(buf[(x0 + 14, y)].symbol(), " ", "message {i}: the text at column 14");
    }
    let small = backend(80, 24, Theme::terminal(), false);
    insta::assert_snapshot!("avatars_off_channel_80x24", mask_hangul(&small.snap()));
}

/// The DMs view in `theme`, the list cursor on row `at`.
fn dms(theme: Theme, avatars: bool) -> Demo {
    let mut d = Demo::with(120, 40, Lang::En, Settings { avatars, ..Settings::default() });
    d.app.theme = theme;
    d.keys("space d");
    assert_eq!(d.app.shell.view, View::Dms);
    d
}

#[test]
fn a_dm_row_is_its_peers_chip_with_the_presence_mark_at_its_corner() {
    let theme = tokyo();
    let d = dms(theme.clone(), true);
    let buf = d.buffer();
    let list = screen::inner(d.app.areas().list.unwrap());
    let rows = d.app.shell.rows(&d.app.model);
    let mut seen_group = false;
    for (k, row) in rows.iter().enumerate() {
        let Row::Conversation(i) = row else { continue };
        let c = d.app.model.conversation(*i);
        let y = list.y + k as u16;
        let x = list.x + 1;
        match &c.kind {
            ConversationKind::Dm { user } => {
                let u = d.app.model.user(user).unwrap();
                let slot = avatar::slot(user) % theme.avatars.len();
                // A muted DM's chip is muted, on the group chip's background.
                let bg = if c.muted { theme.avatar_muted().bg } else { theme.avatar(slot).bg };
                let bg = bg.unwrap();
                if k != d.app.shell.list_cursor || !c.muted {
                    assert_eq!(buf[(x, y)].bg, bg, "{}", c.name.line());
                }
                let want = avatar::initials(u.display_name.line().as_str(), String::new);
                assert_eq!(text(&buf, x, y, 2), want, "{}", c.name.line());
                let mark = buf[(x + 2, y)].symbol();
                assert!(["●", "○", "◐"].contains(&mark), "{}: {mark:?}", c.name.line());
                assert_eq!(buf[(x + 3, y)].symbol(), " ");
                let name: String = u.display_name.line().as_str().chars().take(1).collect();
                assert_eq!(buf[(x + 4, y)].symbol(), name, "{}", c.name.line());
            }
            ConversationKind::GroupDm { users } => {
                seen_group = true;
                assert_eq!(text(&buf, x, y, 2), format!("{:<2}", users.len()));
                assert_eq!(Some(buf[(x, y)].bg), theme.avatar_group().bg, "a neutral chip");
            }
            ConversationKind::Channel { .. } => panic!("DMs lists no channels"),
        }
    }
    assert!(seen_group);
    // The selected row keeps the chip's color and the mark's.
    let selected = list.y + d.app.shell.list_cursor as u16;
    assert_eq!(buf[(list.x + 5, selected)].bg, theme.selection);
    assert!(is_chip(&theme, buf[(list.x + 1, selected)].bg), "the chip stays on the bar");
    assert_eq!(buf[(list.x + 3, selected)].fg, theme.success, "Minsu Kim is active");
    insta::assert_snapshot!("avatars_dms_tokyo_120x40", mask_hangul(&d.screen()));
}

#[test]
fn without_colors_a_chip_is_reversed_and_presence_still_has_its_shape() {
    let d = dms(Theme::no_color(), true);
    let buf = d.buffer();
    let list = screen::inner(d.app.areas().list.unwrap());
    // A row not muted below the selected one (that one is reversed as a whole).
    let rows = d.app.shell.rows(&d.app.model);
    let k = (1..rows.len())
        .find(|&k| matches!(rows[k], Row::Conversation(i) if !d.app.model.conversation(i).muted))
        .unwrap();
    let y = list.y + k as u16;
    assert!(buf[(list.x + 1, y)].modifier.contains(Modifier::REVERSED | Modifier::BOLD));
    assert!(!buf[(list.x + 4, y)].modifier.contains(Modifier::REVERSED), "the name is not");
    assert!(["●", "○", "◐"].contains(&buf[(list.x + 3, y)].symbol()));
    // Off: the mark alone starts the row, as before.
    let d = dms(Theme::no_color(), false);
    let buf = d.buffer();
    assert!(["●", "○", "◐"].contains(&buf[(list.x + 1, y)].symbol()));
    let Row::Conversation(i) = rows[k] else { unreachable!() };
    let name = d.app.model.conversation(i).name.line();
    assert_eq!(text(&buf, list.x + 3, y, 2).chars().next(), name.as_str().chars().next());
}

#[test]
fn a_dm_title_starts_with_the_peers_chip() {
    let theme = tokyo();
    let mut d = dms(theme.clone(), true);
    d.keys("enter");
    let buf = d.buffer();
    let (main, _) = d.app.panes();
    let top = main.unwrap().y;
    let line: String = (0..d.app.size.width).map(|x| buf[(x, top)].symbol().to_string()).collect();
    assert!(line.contains("▌MK @Minsu Kim ● active"), "{line}");
    let x = line[..line.find("▌MK").unwrap()].chars().count() as u16 + 1;
    let me = UserId::new("UDEMOA001");
    assert_eq!(Some(buf[(x, top)].bg), theme.avatar(avatar::slot(&me)).bg);
    let mut small = dms(Theme::terminal(), true);
    small.app.resize(100, 30);
    small.keys("enter");
    insta::assert_snapshot!("avatars_dm_open_100x30", mask_hangul(&small.snap()));
    let mut narrow = dms(Theme::terminal(), true);
    narrow.app.resize(80, 24);
    narrow.keys("enter");
    insta::assert_snapshot!("avatars_dm_open_80x24", mask_hangul(&narrow.snap()));
}

fn saved(d: &mut Demo) -> Vec<String> {
    d.app
        .take_effects()
        .into_iter()
        .filter_map(|e| match e {
            Effect::Save { key: "avatars", value } => Some(value),
            _ => None,
        })
        .collect()
}

#[test]
fn colon_avatars_turns_them_off_and_on_and_asks_to_save_it() {
    let mut d = Demo::new(120, 40);
    assert!(d.app.settings.avatars, "on by default");
    d.command("avatars");
    assert!(!d.app.settings.avatars, ":avatars alone toggles");
    assert!(d.status_line().contains("Avatars: off"), "{}", d.status_line());
    d.command("set avatars=initials");
    assert!(d.app.settings.avatars);
    d.command("avatars off");
    assert!(!d.app.settings.avatars);
    assert_eq!(saved(&mut d), ["off", "initials", "off"]);
    // The palette lists the values, the current one marked.
    d.keys(":");
    d.type_text("avatars ");
    let rows = d.app.palette_rows();
    let named: Vec<(&str, &str)> = rows.iter().map(|r| (r.name.as_str(), r.keys.as_str())).collect();
    assert_eq!(named, [("initials", ""), ("off", "current")]);
    d.keys("enter");
    assert!(d.app.settings.avatars, "Enter picks the selected value");
    // A value it does not take changes nothing and says which ones it takes.
    d.keys(":");
    d.type_text("avatars photo");
    d.keys("enter");
    assert_eq!(d.app.mode(), Mode::Command);
    let s = d.screen();
    assert!(s.contains("Unknown avatars setting photo") && s.contains("initials, off"), "{s}");
    assert!(d.app.settings.avatars);
    assert_eq!(saved(&mut d), ["initials"]);
}

#[test]
fn a_stacked_message_indents_its_text_under_the_name() {
    // 80x24 stacks the main pane: name and time, then the text below the name (after the chip).
    for (avatars, indent) in [(true, 3u16), (false, 2)] {
        let d = backend(80, 24, Theme::terminal(), avatars);
        let buf = d.buffer();
        let (main, _) = d.app.panes();
        let x0 = screen::inner(main.unwrap()).x + 1;
        let pane = d.app.work.main.as_ref().unwrap();
        let hits: Vec<(usize, u16)> = pane.hits.borrow().iter().map(|h| (h.message, h.y)).collect();
        let mut checked = 0;
        for i in hits.iter().map(|h| h.0).collect::<std::collections::BTreeSet<_>>() {
            // The message's rows, a date rule left out: a header (from the first cell) when it
            // starts a group, then its text.
            let rows: Vec<u16> =
                hits.iter().filter(|h| h.0 == i && buf[(x0, h.1)].symbol() != "─").map(|h| h.1).collect();
            let text_row = match rows.first() {
                Some(&y) if buf[(x0, y)].symbol() != " " => rows.get(1).copied(),
                first => first.copied(),
            };
            let Some(y) = text_row else { continue };
            if pane.items[i].text.as_str().trim().is_empty() {
                continue;
            }
            assert_eq!(text(&buf, x0, y, indent).trim(), "", "message {i}: indent {indent}");
            assert_ne!(buf[(x0 + indent, y)].symbol(), " ", "message {i}: text at {indent}");
            checked += 1;
        }
        assert!(checked >= 3, "{checked}");
    }
}
