//! A read conversation's name is muted, an unread one's bold in body text, a muted
//! conversation's faint whatever it holds: in every section and in the DMs view, in a built-in
//! theme and in a theme file. The cursor's bar lifts a read name to body text, never to bold; the
//! row under the mouse keeps its name's colour.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::buffer::{Buffer, Cell};
use ratatui::crossterm::event::MouseEventKind;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use slakio_tui::app::model::Row;
use slakio_tui::app::shell::View;
use slakio_tui::theme::{Background, Look, Theme, resolve};

const SIZES: [(u16, u16); 2] = [(80, 24), (120, 40)];

/// Dark, light, and a theme file whose body text is near dark's muted colour.
fn themes() -> Vec<Theme> {
    let dir = std::env::temp_dir().join(format!("slakio-list-read-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("dimtext.toml"), "extends = \"dark\"\n[colors]\nfg = \"#9aa3b5\"\n").unwrap();
    let look = Look { truecolor: true, background: Background::Dark, no_color: false };
    let custom = slakio_tui::theme::load("dimtext", look, Some(&dir));
    let _ = std::fs::remove_dir_all(&dir);
    vec![
        resolve("dark", true, Background::Dark),
        resolve("light", true, Background::Light),
        custom.expect("the theme file is good"),
    ]
}

/// How a conversation's name is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Unread,
    Read,
    Muted,
}

fn state(d: &Demo, i: usize) -> State {
    let c = d.app.model.conversation(i);
    match (c.muted, c.unread > 0) {
        (true, _) => State::Muted,
        (false, true) => State::Unread,
        (false, false) => State::Read,
    }
}

fn rows_area(d: &Demo) -> Rect {
    d.app.list_parts().expect("the list panel").rows
}

/// The screen row of list row `k`, when it is shown.
fn y_of(d: &Demo, k: usize) -> Option<u16> {
    let a = rows_area(d);
    let top = d.app.list_top();
    (k >= top && k - top < usize::from(a.height)).then(|| a.y + (k - top) as u16)
}

/// The cells of the name of conversation `i` drawn on screen row `y` (its first characters, the
/// names that start with ASCII).
fn name_cells(d: &Demo, buf: &Buffer, i: usize, y: u16) -> Option<Vec<Cell>> {
    let name = d.app.model.conversation(i).name.line().into_string();
    let head: Vec<String> = name.chars().take(3).map(String::from).collect();
    if head.len() < 3 || !name.is_char_boundary(3) || !name[..3].is_ascii() {
        return None;
    }
    let a = rows_area(d);
    (a.left()..a.right().saturating_sub(3))
        .find(|&x| (0..3).all(|j| buf[(x + j, y)].symbol() == head[j as usize]))
        .map(|x| (x..x + 3).map(|x| buf[(x, y)].clone()).collect())
}

/// The conversations shown, not under the cursor: their list row, index and screen row.
fn shown(d: &Demo) -> Vec<(usize, usize, u16)> {
    let rows = d.app.list_rows();
    (0..rows.len())
        .filter(|&k| k != d.app.list_cursor())
        .filter_map(|k| match rows[k] {
            Row::Conversation(i) => y_of(d, k).map(|y| (k, i, y)),
            _ => None,
        })
        .collect()
}

/// The demo in `t` at `size` on `view`, its cursor on the first row.
fn demo(t: &Theme, (w, h): (u16, u16), view: View) -> Demo {
    let mut d = Demo::new(w, h);
    d.app.theme = t.clone();
    d.app.show_unfolded(view);
    d.screen();
    d
}

fn bold(c: &Cell) -> bool {
    c.modifier.contains(Modifier::BOLD)
}

#[test]
fn read_and_unread_rows_have_their_exact_cells() {
    for size in SIZES {
        for t in themes() {
            for view in [View::Home, View::Dms] {
                let what = format!("{} {size:?} {view:?}", t.name);
                let d = demo(&t, size, view);
                let buf = d.buffer();
                let mut seen = Vec::new();
                for (_, i, y) in shown(&d) {
                    let Some(cells) = name_cells(&d, &buf, i, y) else { continue };
                    let s = state(&d, i);
                    seen.push(s);
                    let name = d.app.model.conversation(i).name.line().into_string();
                    for c in &cells {
                        let want = match s {
                            State::Unread => (t.fg, true),
                            State::Read => (t.fg_muted, false),
                            State::Muted => (t.fg_dim, false),
                        };
                        assert_eq!((c.fg, bold(c)), want, "{what}: {name} {s:?}");
                        assert_eq!(c.bg, t.bg, "{what}: {name}");
                    }
                }
                for s in [State::Unread, State::Read] {
                    assert!(seen.contains(&s), "{what}: a {s:?} row is shown: {seen:?}");
                }
            }
        }
    }
}

#[test]
fn selected_and_hovered_rows_stay_apart_on_read_and_unread() {
    for size in SIZES {
        for t in themes() {
            let what = format!("{} {size:?}", t.name);
            let d = demo(&t, size, View::Home);
            let buf = d.buffer();
            let pick = |s: State| {
                shown(&d)
                    .into_iter()
                    .find(|&(_, i, y)| state(&d, i) == s && name_cells(&d, &buf, i, y).is_some())
                    .unwrap_or_else(|| panic!("{what}: no {s:?} row shown"))
            };
            let (read, unread) = (pick(State::Read), pick(State::Unread));
            let mut looks = Vec::new();
            for (s, (k, i, _)) in [(State::Read, read), (State::Unread, unread)] {
                // Under the cursor: the bar, body text; bold only when unread.
                let mut d = demo(&t, size, View::Home);
                d.app.focus_list_row(k);
                let buf = d.buffer();
                let y = y_of(&d, k).unwrap();
                let cells = name_cells(&d, &buf, i, y).unwrap();
                for c in &cells {
                    assert_eq!(
                        (c.fg, c.bg, bold(c)),
                        (t.fg, t.selection, s == State::Unread),
                        "{what}: {s:?} selected"
                    );
                }
                looks.push(("selected", s, cells[0].clone()));
                // Under the mouse: the hover colour, the name's own colour and weight.
                let mut d = demo(&t, size, View::Home);
                let y = y_of(&d, k).unwrap();
                d.mouse(MouseEventKind::Moved, rows_area(&d).x + 3, y);
                let buf = d.buffer();
                let cells = name_cells(&d, &buf, i, y).unwrap();
                let fg = if s == State::Unread { t.fg } else { t.fg_muted };
                for c in &cells {
                    assert_eq!((c.fg, c.bg, bold(c)), (fg, t.hover, s == State::Unread), "{what}: {s:?} hovered");
                }
                looks.push(("hovered", s, cells[0].clone()));
            }
            // A read row and an unread one never look alike, selected or hovered.
            for (how, _, a) in looks.iter().filter(|l| l.1 == State::Read) {
                let (_, _, b) = looks.iter().find(|l| l.0 == *how && l.1 == State::Unread).unwrap();
                assert!((a.fg, bold(a)) != (b.fg, bold(b)), "{what}: {how} read and unread apart");
                assert_eq!(a.bg, b.bg, "{what}: {how} on the same background");
            }
            // The selection and the hover look apart from each other.
            assert_ne!(looks[0].2.bg, looks[1].2.bg, "{what}: selected and hovered");
        }
    }
}
