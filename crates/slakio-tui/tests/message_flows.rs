//! The comfortable message layout (the default `density`): the sender's picture in a 4 × 2 slot,
//! the name and time over the text, groups without a header, one blank row between blocks (and
//! after reactions), reactions as emoji chips, the selection over the whole block, and the
//! switch to the compact columns with `:density`.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use slakio_core::i18n::Lang;
use slakio_tui::app::{Effect, PaneKind, Settings};
use slakio_tui::screen;
use slakio_tui::theme::{Background, Kind, resolve};

/// The text of row `y` from `x` for `w` cells.
fn text(buf: &Buffer, x: u16, y: u16, w: u16) -> String {
    let mut out = String::new();
    let mut skip = 0;
    for x in x..x + w {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        let s = buf[(x, y)].symbol();
        out.push_str(s);
        skip = slakio_tui::text::width(s).saturating_sub(1);
    }
    out
}

/// The demo in tokyo-night with `name` open, `w` × `h`.
fn open(name: &str, w: u16, h: u16) -> Demo {
    let mut d = Demo::new(w, h);
    d.app.theme = resolve("tokyo-night", true, Background::Dark);
    d.open(name);
    d
}

/// The messages area of the conversation and the first row of each message drawn there (its
/// header, or its text in a group), top down; the date rules left out.
fn firsts(d: &Demo) -> (Rect, Vec<(usize, u16)>) {
    let _ = d.buffer();
    let pane = d.pane(PaneKind::Conversation).unwrap();
    let area = d.app.message_area(pane.handle()).unwrap();
    let buf = d.buffer();
    let mut out: Vec<(usize, u16)> = Vec::new();
    for (i, y) in pane.drawn_rows() {
        if buf[(area.x + 3, y)].symbol() == "─" || out.iter().any(|(m, _)| *m == i) {
            continue;
        }
        out.push((i, y));
    }
    (area, out)
}

#[test]
fn a_group_starts_with_the_picture_and_the_name_over_the_text() {
    let d = open("backend", 120, 40);
    let t = d.app.theme.clone();
    let buf = d.buffer();
    let pane = d.pane(PaneKind::Conversation).unwrap();
    let (area, rows) = firsts(&d);
    let x0 = area.x + 1;
    let mut headers = 0;
    for &(i, y) in rows.iter().filter(|(_, y)| *y > area.y && *y + 1 < area.bottom()) {
        let m = &pane.messages()[i];
        let picture = buf[(x0, y)].bg;
        if !t.avatars.iter().any(|_| true) || buf[(x0 + 6, y)].symbol() == " " {
            continue;
        }
        let header = text(&buf, x0 + 6, y, 60);
        if !header.starts_with(&m.author.as_str().chars().take(3).collect::<String>()) {
            // A grouped message: its text where the text goes, no picture.
            assert_eq!(text(&buf, x0, y, 6).trim(), "", "message {i}: no picture in a group");
            continue;
        }
        headers += 1;
        assert!(buf[(x0 + 6, y)].modifier.contains(Modifier::BOLD), "message {i}: the name is bold");
        assert!(header.contains(&slakio_tui::time::hm(m.ts)), "message {i}: its time beside the name: {header}");
        // The picture: 4 × 2 cells of one tint, the initials on the first row.
        for (dx, dy) in [(0, 0), (3, 0), (0, 1), (3, 1)] {
            assert_eq!(buf[(x0 + dx, y + dy)].bg, picture, "message {i}: the slot's cell {dx},{dy}");
        }
        assert_ne!(picture, t.bg, "message {i}: tinted");
        // The text right under the name, using the rest of the width.
        if !m.text.as_str().trim().is_empty() {
            assert_ne!(buf[(x0 + 6, y + 1)].symbol(), " ", "message {i}: its text under the name");
        }
    }
    assert!(headers >= 3, "{headers}");
}

#[test]
fn blocks_are_one_blank_row_apart_and_reactions_never_touch_the_next_message() {
    for name in ["general", "backend", "incidents"] {
        let d = open(name, 120, 40);
        let pane = d.pane(PaneKind::Conversation).unwrap();
        let (area, rows) = firsts(&d);
        let buf = d.buffer();
        let blank = |y: u16| text(&buf, area.x, y, area.width).trim().is_empty();
        for w in rows.windows(2) {
            let ((a, _), (b, yb)) = (w[0], w[1]);
            let starts_group = buf[(area.x + 7, yb)].modifier.contains(Modifier::BOLD);
            let after_reactions = !pane.messages()[a].reactions.is_empty();
            let new_day = slakio_tui::time::day(pane.messages()[a].ts) != slakio_tui::time::day(pane.messages()[b].ts);
            if (starts_group || after_reactions) && !new_day {
                assert!(blank(yb - 1), "{name}: one blank row above message {b}");
                assert!(!blank(yb - 2), "{name}: never two, above message {b}");
            }
        }
        // Nothing blank at the bottom, by the composer.
        assert!(!blank(area.bottom() - 1), "{name}: the newest message ends the area");
    }
}

#[test]
fn reactions_are_raised_emoji_chips_at_the_text_and_the_users_own_are_filled_with_the_accent() {
    let d = open("general", 160, 50);
    let t = d.app.theme.clone();
    let pane = d.pane(PaneKind::Conversation).unwrap();
    let area = d.app.message_area(pane.handle()).unwrap();
    let buf = d.buffer();
    let s = d.screen();
    let mut seen = 0;
    for m in pane.messages().iter().rev().take(20) {
        for r in &m.reactions {
            let Some(e) = slakio_tui::emoji::get(r.name.as_str()) else { continue };
            let chip = format!(" {e} {} ", r.count);
            // Every place this chip is drawn; one of them is this message's.
            let at: Vec<(u16, u16)> = (0..buf.area.height)
                .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    buf[(x, y)].symbol().starts_with(e)
                        && text(&buf, x.saturating_sub(1), y, 6).contains(chip.trim_end())
                })
                .collect();
            if at.is_empty() {
                continue;
            }
            seen += 1;
            let ok = at.iter().any(|&(x, y)| {
                let c = &buf[(x, y)];
                let pill = if r.mine { c.bg == t.accent } else { c.bg == t.raised() && c.bg != t.bg };
                // The first chip's pad sits in the gap: its emoji lines up with the text (the
                // gutter, the picture's four cells and two blank ones).
                let first = buf[(x - 2, y)].bg == t.bg && buf[(x - 3, y)].bg == t.bg;
                pill && (!first || x == area.x + 7)
            });
            assert!(
                ok,
                "{chip}: {}",
                if r.mine { "mine filled with the accent" } else { "a raised pill, at the text" }
            );
        }
    }
    assert!(seen >= 2, "{seen}: {s}");
    assert!(!s.contains(":+1:") && !s.contains(":eyes:"), "standard names are emoji: {s}");
}

#[test]
fn a_grouped_message_shows_its_time_only_while_selected_and_the_selection_covers_the_block() {
    let mut d = open("backend", 120, 40);
    let t = d.app.theme.clone();
    let msgs = d.pane(PaneKind::Conversation).unwrap().messages().to_vec();
    let n = msgs.len();
    // The newest message that is in a group (no header), and one with reactions.
    let grouped = (1..n).rev().find(|&i| {
        let (a, b) = (&msgs[i - 1], &msgs[i]);
        a.user == b.user
            && b.ts.0 - a.ts.0 <= 300_000_000
            && a.thread.is_none()
            && slakio_tui::time::day(a.ts) == slakio_tui::time::day(b.ts)
    });
    if let Some(i) = grouped {
        let hm = slakio_tui::time::hm(msgs[i].ts);
        d.app.select_message(i);
        let rows: Vec<u16> = {
            let _ = d.buffer();
            d.pane(PaneKind::Conversation)
                .unwrap()
                .drawn_rows()
                .into_iter()
                .filter(|(m, _)| *m == i)
                .map(|(_, y)| y)
                .collect()
        };
        let buf = d.buffer();
        let x0 = d.app.message_area(d.pane(PaneKind::Conversation).unwrap().handle()).unwrap().x + 1;
        assert_eq!(text(&buf, x0, rows[0], 5), hm, "its time in the slot's place while selected");
        d.keys("esc");
    }
    let i = (0..n).rev().find(|&i| !msgs[i].reactions.is_empty()).expect("a message with reactions");
    d.app.select_message(i);
    let _ = d.buffer();
    let p = d.pane(PaneKind::Conversation).unwrap();
    let area = d.app.message_area(p.handle()).unwrap();
    let rows: Vec<u16> = p.drawn_rows().into_iter().filter(|(m, _)| *m == i).map(|(_, y)| y).collect();
    let buf = d.buffer();
    assert!(rows.len() >= 3, "{rows:?}");
    for &y in &rows {
        if buf[(area.x + 3, y)].symbol() == "─" {
            continue;
        }
        assert_eq!(buf[(area.right() - 2, y)].bg, t.selection, "row {y}: the whole block is selected");
    }
    let below = rows.iter().max().unwrap() + 1;
    if below < area.bottom() {
        assert_ne!(buf[(area.right() - 2, below)].bg, t.selection, "the blank after it is not");
    }
}

#[test]
fn density_switches_to_columns_and_back_and_is_saved() {
    let mut d = open("backend", 120, 40);
    assert!(!d.app.settings.compact, "comfortable is the default");
    d.command("density");
    assert!(d.app.settings.compact);
    assert!(d.app.take_effects().contains(&Effect::Save { key: "density", value: "compact".to_string() }));
    let s = d.screen();
    let pane = d.pane(PaneKind::Conversation).unwrap();
    let last = pane.messages().last().unwrap();
    let line = s.lines().find(|l| l.contains(&slakio_tui::time::hm(last.ts))).unwrap_or_default().to_string();
    assert!(
        line.trim_end().trim_end_matches('│').trim_end().ends_with(&slakio_tui::time::hm(last.ts)),
        "compact: the time on the right: {line}"
    );
    d.command("density comfortable");
    assert!(!d.app.settings.compact);
    d.command("set density=compact");
    assert!(d.app.settings.compact);
    d.command("density cozy");
    assert!(d.app.settings.compact, "a value it does not take changes nothing");
}

#[test]
fn narrow_panes_shrink_the_picture_then_leave_it_out() {
    // 80 columns with a thread open: the thread panel is narrow, its replies take the 2-cell chip.
    let mut d = Demo::with(80, 24, Lang::En, Settings::default());
    d.open("long-threads");
    d.keys("g g enter");
    let thread = d.app.message_area(d.pane(PaneKind::Thread).unwrap().handle()).unwrap();
    assert!(thread.width < 56);
    let buf = d.buffer();
    let s: Vec<String> = (thread.y..thread.bottom()).map(|y| text(&buf, thread.x + 1, y, thread.width - 1)).collect();
    assert!(s.iter().any(|l| l.len() > 4 && l.chars().nth(2) == Some(' ') && l.chars().nth(3) == Some(' ')), "{s:#?}");
    // In the 16-color theme the picture has no solid background.
    let mut d = Demo::new(120, 40);
    d.app.theme = resolve("terminal", false, Background::Dark);
    d.open("backend");
    let area = screen::inner(d.pane_area(PaneKind::Conversation).unwrap());
    let buf = d.buffer();
    assert_eq!(d.app.theme.kind, Kind::Ansi);
    assert!((area.y..area.bottom()).all(|y| buf[(area.x + 1, y)].bg == ratatui::style::Color::Reset), "no solid block");
}
