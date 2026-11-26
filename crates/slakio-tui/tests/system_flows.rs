//! Slack's notices about a conversation (joined, left, topic, rename, archive, pin, app, private)
//! as one quiet line: an icon in the gap before the text column, the words in muted text, the
//! time after them; no picture, no name, never grouped with the messages around it, its files,
//! "N replies" link and reactions kept under it; selected, hovered and clicked like any message.
//! Comfortable and compact, icons on and off, English and Korean, at 80x24 and 120x40.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use demo::assert_harmless;
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use scripted::Scripted;
use slakio_core::backend::Event;
use slakio_core::i18n::{I18n, Lang, Msg};
use slakio_core::model::{Message, Reaction, System, ThreadSummary, Ts, UserId};
use slakio_core::sanitize::Remote;
use slakio_tui::app::PaneKind;
use slakio_tui::theme::{Background, resolve};

const SIZES: [(u16, u16); 2] = [(80, 24), (120, 40)];
const LANGS: [Lang; 2] = [Lang::En, Lang::Ko];
/// 2026-01-05 09:00 UTC.
const AT: u64 = 1_767_603_600;

/// Every look: density (compact) and icons, in each language and size.
fn looks() -> Vec<(Lang, (u16, u16), bool, bool)> {
    let mut out = Vec::new();
    for lang in LANGS {
        for size in SIZES {
            for compact in [false, true] {
                for icons in [false, true] {
                    out.push((lang, size, compact, icons));
                }
            }
        }
    }
    out
}

fn message(secs: u64, user: &UserId, text: &str, system: Option<System>) -> Message {
    Message {
        ts: Ts((AT + secs) * 1_000_000),
        user: user.clone(),
        text: Remote::new(text),
        thread: None,
        reactions: Vec::new(),
        edited: false,
        files: Vec::new(),
        emoji_pictures: Vec::new(),
        system: system.map(Box::new),
    }
}

/// Read-only, booted, `deploys` open in the dark theme in `look`, its history the messages
/// `make` gives for two of its people; the newest selected.
fn shown(look: (Lang, (u16, u16), bool, bool), make: impl Fn(&UserId, &UserId) -> Vec<Message>) -> Scripted {
    let (lang, (w, h), compact, icons) = look;
    let mut s = Scripted::booted(true, None);
    s.d.app.i18n = I18n::new(lang);
    s.d.app.theme = resolve("dark", true, Background::Dark);
    s.d.app.settings.compact = compact;
    s.d.app.settings.icons = icons;
    s.d.app.resize(w, h);
    s.d.list_cursor_on("deploys");
    s.keys("enter");
    let mut e = s.b.reply(0);
    let Event::History(page) = &mut e.event else { panic!("a page") };
    let a = page.messages[0].user.clone();
    let b = page.messages.iter().map(|m| m.user.clone()).find(|u| *u != a).expect("two people");
    page.messages = make(&a, &b);
    page.complete = true;
    s.b.push(e);
    s.pump();
    s.keys("G");
    s
}

fn what(look: (Lang, (u16, u16), bool, bool)) -> String {
    format!("{:?} {:?} compact={} icons={}", look.0, look.1, look.2, look.3)
}

fn author(s: &Scripted, i: usize) -> String {
    s.d.pane(PaneKind::Conversation).unwrap().messages()[i].author.as_str().to_string()
}

/// The screen rows of message `i` (its rules left out) and the pane's lines, by screen row.
fn rows(s: &Scripted, i: usize) -> (Vec<u16>, Vec<String>) {
    let buf = s.d.buffer();
    let a = s.d.app.message_area(s.d.pane(PaneKind::Conversation).unwrap().handle()).unwrap();
    let lines: Vec<String> = (0..buf.area.height)
        .map(|y| {
            if y < a.y || y >= a.bottom() {
                return String::new();
            }
            // A wide character's second cell is drawn by its first.
            let mut out = String::new();
            let mut skip = 0;
            for x in a.x..a.right() {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let sym = buf[(x, y)].symbol();
                skip = unicode_width::UnicodeWidthStr::width(sym).saturating_sub(1);
                out.push_str(sym);
            }
            out
        })
        .collect();
    let p = s.d.pane(PaneKind::Conversation).unwrap();
    let ys = p
        .drawn_rows()
        .into_iter()
        .filter(|(m, y)| *m == i && !lines[usize::from(*y)].contains('\u{2500}'))
        .map(|(_, y)| y)
        .collect();
    (ys, lines)
}

fn hm(s: &Scripted, secs: u64) -> String {
    slakio_tui::time::hm(Ts((AT + secs) * 1_000_000), &s.d.app.zone)
}

#[test]
fn a_system_row_is_one_muted_line_without_picture_or_name() {
    for look in looks() {
        let at = what(look);
        // A message after it holds the selection, so the notice shows its own colours.
        let s = shown(look, |a, b| {
            vec![message(120, a, "<@U> has left the channel", Some(System::Left)), message(600, b, "Bye", None)]
        });
        let name = author(&s, 0);
        let words = s.d.app.i18n.msg(&Msg::SystemLeft { name: name.clone(), channel: "#deploys".into() }).to_string();
        let icon = if look.3 { "\u{F0015}" } else { "-" };
        let (ys, lines) = rows(&s, 0);
        assert_eq!(ys.len(), 1, "{at}: one row");
        let line = &lines[usize::from(ys[0])];
        // Whole where it fits (a narrow pane cuts the words, never the time).
        let time = hm(&s, 120);
        let want = if look.1.0 >= 120 { format!(" {icon} {words}  {time}") } else { format!(" {icon} ") };
        let start = line.find(&want).unwrap_or_else(|| panic!("{at}: {line:?} lacks {want:?}"));
        assert!(line[..start].chars().all(|c| c == ' '), "{at}: no picture before it: {line:?}");
        let head: String = words.chars().take(10).collect();
        assert!(line[start..].starts_with(&format!(" {icon} {head}")), "{at}: {line:?}");
        assert!(line.trim_end().ends_with(&format!("  {time}")), "{at}: {line:?}");
        if look == (Lang::En, (120, 40), false, false) {
            // The gutter's cell, then the icon two cells before the text column (6 in).
            assert!(line.starts_with(&format!("     - {name} left #deploys.  {time}")), "{at}: {line:?}");
        }
        let a = s.d.app.message_area(s.d.pane(PaneKind::Conversation).unwrap().handle()).unwrap();
        assert_eq!(lines.iter().filter(|l| l.contains(&name)).count(), 1, "{at}: no name header");
        // The words in the muted colour, the time in the faint one.
        let buf = s.d.buffer();
        let t = &s.d.app.theme;
        let y = ys[0];
        let marks: Vec<u16> = (a.x + 1..a.right()).filter(|&x| !matches!(buf[(x, y)].symbol(), " " | "")).collect();
        assert_eq!(buf[(marks[0], y)].fg, t.muted().fg.unwrap(), "{at}: the icon is muted");
        assert_eq!(buf[(marks[1], y)].fg, t.muted().fg.unwrap(), "{at}: the words are muted");
        assert_eq!(buf[(*marks.last().unwrap(), y)].fg, t.faint().fg.unwrap(), "{at}: the time is faint");
        assert_harmless(&s.d.screen());
    }
}

#[test]
fn a_system_row_keeps_its_chips_and_reply_link() {
    for look in looks() {
        let at = what(look);
        let s = shown(look, |a, _| {
            let mut m = message(60, a, "", Some(System::Joined));
            m.thread = Some(ThreadSummary { replies: 3, last_reply: Ts((AT + 300) * 1_000_000) });
            m.reactions = vec![Reaction { name: Remote::new("wave"), count: 4, mine: false }];
            vec![m]
        });
        let (ys, lines) = rows(&s, 0);
        assert_eq!(ys.len(), 3, "{at}: the notice, the link, the chips: {ys:?}");
        let link = s.d.app.i18n.msg(&Msg::MessageReplies { count: 3, time: hm(&s, 300) }).to_string();
        assert!(lines[usize::from(ys[1])].contains(&link), "{at}: {:?}", lines[usize::from(ys[1])]);
        assert!(lines[usize::from(ys[2])].contains('4'), "{at}: {:?}", lines[usize::from(ys[2])]);
    }
}

#[test]
fn a_system_row_after_its_author_s_message_does_not_group() {
    for look in looks() {
        let at = what(look);
        let s =
            shown(look, |a, _| vec![message(0, a, "Deploying now", None), message(30, a, "", Some(System::Pinned))]);
        let (first, _) = rows(&s, 0);
        let (ys, lines) = rows(&s, 1);
        assert_eq!(ys.len(), 1, "{at}");
        assert_eq!(
            ys[0],
            first.last().unwrap() + 2,
            "{at}: one blank row between: {first:?} {ys:?}\n{}",
            lines.join("\n")
        );
        assert!(lines[usize::from(ys[0] - 1)].trim().is_empty(), "{at}");
        let words = s.d.app.i18n.msg(&Msg::SystemPinned { name: author(&s, 1) }).to_string();
        let words: String = words.chars().take(12).collect();
        assert!(lines[usize::from(ys[0])].contains(&words), "{at}: {:?}", lines[usize::from(ys[0])]);
    }
}

#[test]
fn a_message_after_a_system_row_starts_a_new_block() {
    for look in looks() {
        let at = what(look);
        let s =
            shown(look, |a, _| vec![message(0, a, "", Some(System::Joined)), message(30, a, "Hello everyone", None)]);
        let (sys, _) = rows(&s, 0);
        let (ys, lines) = rows(&s, 1);
        assert_eq!(ys[0], sys[0] + 2, "{at}: a blank row between");
        let name = author(&s, 1);
        let shown: String = ys.iter().map(|y| lines[usize::from(*y)].clone()).collect::<Vec<_>>().join("\n");
        assert!(shown.contains(&name), "{at}: the name heads the message:\n{shown}");
    }
}

#[test]
fn two_system_rows_in_a_row_have_no_blank_between() {
    for look in looks() {
        let at = what(look);
        let s = shown(look, |a, b| {
            vec![
                message(0, a, "", Some(System::Joined)),
                message(10, b, "", Some(System::Left)),
                message(20, a, "", Some(System::Topic(Remote::new("Deploy days")))),
            ]
        });
        let (one, _) = rows(&s, 0);
        let (two, _) = rows(&s, 1);
        let (three, lines) = rows(&s, 2);
        assert_eq!((two[0], three[0]), (one[0] + 1, one[0] + 2), "{at}: one after another\n{}", lines.join("\n"));
        let name: String = author(&s, 2).chars().take(4).collect();
        assert!(lines[usize::from(three[0])].contains(&name), "{at}: {:?}", lines[usize::from(three[0])]);
    }
}

#[test]
fn a_system_row_is_selectable_and_clickable() {
    for look in looks() {
        let at = what(look);
        let mut s = shown(look, |a, b| {
            vec![
                message(0, a, "Deploying now", None),
                message(400, b, "", Some(System::Joined)),
                message(800, a, "Done", None),
            ]
        });
        let t = s.d.app.theme.clone();
        let painted = |s: &Scripted, y: u16, bg| {
            let buf = s.d.buffer();
            (buf.area.left()..buf.area.right()).any(|x| buf[(x, y)].bg == bg)
        };
        s.keys("k");
        assert_eq!(s.d.pane(PaneKind::Conversation).unwrap().selected(), Some(1), "{at}: k selects it");
        let (ys, _) = rows(&s, 1);
        assert!(painted(&s, ys[0], t.selection), "{at}: drawn selected");
        s.keys("G");
        let (ys, _) = rows(&s, 1);
        let a = s.d.app.message_area(s.d.pane(PaneKind::Conversation).unwrap().handle()).unwrap();
        assert!(s.d.mouse(MouseEventKind::Moved, a.x + 4, ys[0]), "{at}: a move onto it draws");
        if t.hover != ratatui::style::Color::Reset {
            assert!(painted(&s, ys[0], t.hover), "{at}: drawn hovered");
        }
        s.d.mouse(MouseEventKind::Down(MouseButton::Left), a.x + 4, ys[0]);
        s.d.mouse(MouseEventKind::Up(MouseButton::Left), a.x + 4, ys[0]);
        assert_eq!(s.d.pane(PaneKind::Conversation).unwrap().range(), Some((1, 1)), "{at}: a click selects it");
    }
}

#[test]
fn a_long_topic_is_clipped_and_keeps_its_time() {
    for look in looks() {
        let at = what(look);
        let topic = "Release train every Tuesday, freeze on Monday noon, ".repeat(6);
        let s = shown(look, |a, _| vec![message(90, a, "", Some(System::Topic(Remote::new(topic.clone()))))]);
        let (ys, lines) = rows(&s, 0);
        assert_eq!(ys.len(), 1, "{at}");
        let line = lines[usize::from(ys[0])].trim_end();
        assert!(line.contains('\u{2026}'), "{at}: cut: {line:?}");
        assert!(line.ends_with(&format!("\u{2026}  {}", hm(&s, 90))), "{at}: the time after it: {line:?}");
    }
}
