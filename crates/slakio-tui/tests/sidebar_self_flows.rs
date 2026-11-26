//! The list's DM rows after the name: on the user's own DM, `you` (its Korean word in ko) in muted,
//! then the peer's status emoji (a standard one as its glyph, one of the workspace's own as its
//! picture, its cells keeping the colour that names it, else `:name:`); a status past its end
//! is not shown, and the user's own account unknown, no row says `you`. Short of room the
//! status goes first, then `you`, then the name is cut. At 80x24 and 120x40, en and ko.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use demo::Demo;
use ratatui::crossterm::event::MouseEventKind;
use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, Request, Snapshot};
use slakio_core::i18n::Lang;
use slakio_core::model::{ConversationKind, Status, UserId};
use slakio_core::sanitize::Remote;
use slakio_tui::app::shell::View;
use slakio_tui::app::{Effect, Settings};
use slakio_tui::images::graphics::{color, is_placeholder};
use slakio_tui::images::{Look, prepare};
use slakio_tui::theme::{Background, resolve};
use slakio_world::World;

const SIZES: [(u16, u16); 2] = [(80, 24), (120, 40)];
const LANGS: [Lang; 2] = [Lang::En, Lang::Ko];
const TADA: &str = "\u{1F389}";
const PALM: &str = "\u{1F334}";

fn cases() -> impl Iterator<Item = ((u16, u16), Lang)> {
    SIZES.into_iter().flat_map(|s| LANGS.map(|l| (s, l)))
}

fn you(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "you",
        Lang::Ko => "\u{b098}",
    }
}

/// The demo at `w`×`h` in `lang`, its first workspace changed by `change`, the list showing DMs.
fn dms(w: u16, h: u16, lang: Lang, change: impl FnOnce(&mut Snapshot)) -> Demo {
    let mut d = Demo::with(w, h, lang, Settings::default());
    let mut snap = World::demo().snapshot().clone();
    change(&mut snap);
    let ws = snap.workspaces[0].id.clone();
    d.app.model.merge(&ws, &snap, false);
    d.keys("space d");
    assert_eq!(d.app.view(), View::Dms);
    d
}

/// The first workspace's own DM in `snap`, and the user's id.
fn own_dm(snap: &mut Snapshot) -> (&mut slakio_core::model::Conversation, UserId) {
    let me = snap.workspaces[0].me.clone();
    let ws = snap.workspaces[0].id.clone();
    let c = (snap.conversations.iter_mut())
        .find(|c| c.workspace == ws && c.kind == ConversationKind::Dm { user: me.clone() })
        .expect("the demo has the user's own DM");
    (c, me)
}

/// The list row (between its borders) holding `name`, trimmed, its count pill left out.
fn row(screen: &str, name: &str) -> String {
    let line = (screen.lines())
        .map(|l| l.split("││").next().unwrap_or_default().trim_start_matches('│'))
        .find(|l| l.contains(name))
        .unwrap_or_else(|| panic!("no row with {name:?}:\n{screen}"));
    line.trim().to_string()
}

/// Where `text` starts on the screen's row `y` (in cells), and that row.
fn cell_of(d: &Demo, text: &str) -> (u16, u16) {
    let buf = d.buffer();
    for y in 0..buf.area.height {
        let line: Vec<String> = (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
        for x in 0..line.len() {
            if line[x..].concat().starts_with(text) {
                return (x as u16, y);
            }
        }
    }
    panic!("no {text:?} on screen:\n{}", d.screen())
}

#[test]
fn the_own_dm_row_says_you_then_the_status() {
    for ((w, h), lang) in cases() {
        let d = dms(w, h, lang, |_| {});
        let screen = d.screen();
        let at = format!("{w}x{h} {lang:?}:\n{screen}");
        assert_eq!(row(&screen, "Me "), format!("M ● Me {} {TADA}", you(lang)), "{at}");
        // `you` is muted, also beside the name of an unread row.
        let (x, y) = cell_of(&d, &format!("Me {}", you(lang)));
        assert_eq!(d.buffer()[(x + 3, y)].fg, d.app.theme.muted().fg.unwrap(), "{at}");
        let unread = dms(w, h, lang, |s| own_dm(s).0.unread = 1);
        let (x, y) = cell_of(&unread, &format!("Me {}", you(lang)));
        let buf = unread.buffer();
        assert_eq!(buf[(x + 3, y)].fg, d.app.theme.muted().fg.unwrap(), "{at}");
        assert_ne!(buf[(x, y)].style(), buf[(x + 3, y)].style(), "the unread name is not muted: {at}");
        // A peer's row: their status, never `you`.
        assert_eq!(row(&screen, "Minsu Kim"), format!("MK● Minsu Kim {PALM}        3"), "{at}");
        assert_eq!(screen.matches(&format!(" {} ", you(lang))).count(), 1, "{at}");
        // The user's own account unknown: no row says `you` (as before), the status stays.
        let d = dms(w, h, lang, |s| s.workspaces[0].me = UserId::new(""));
        let screen = d.screen();
        assert_eq!(row(&screen, "Me "), format!("M ● Me {TADA}"), "{w}x{h} {lang:?}:\n{screen}");
        // Slack says there is none: the name alone, then `you`.
        let d = dms(w, h, lang, |s| {
            let (_, me) = own_dm(s);
            s.users.iter_mut().filter(|u| u.id == me).for_each(|u| (u.status, u.no_status) = (None, true));
        });
        assert_eq!(row(&d.screen(), "Me "), format!("M ● Me {}", you(lang)), "{w}x{h} {lang:?}");
    }
}

#[test]
fn an_expired_status_is_not_shown() {
    for ((w, h), lang) in cases() {
        let ends = |end: u64| {
            move |s: &mut Snapshot| {
                for u in s.users.iter_mut().filter(|u| u.status.is_some()) {
                    u.status = Some(Status { expires: Some(end), ..u.status.clone().unwrap() });
                }
            }
        };
        let d = dms(w, h, lang, ends(1));
        let screen = d.screen();
        let at = format!("{w}x{h} {lang:?}:\n{screen}");
        assert_eq!(row(&screen, "Me "), format!("M ● Me {}", you(lang)), "{at}");
        assert_eq!(row(&screen, "Minsu Kim"), "MK● Minsu Kim           3", "{at}");
        assert!(!screen.contains(TADA) && !screen.contains(PALM) && !screen.contains(":party-blob:"), "{at}");
        // One that ends later than now still shows.
        let d = dms(w, h, lang, ends(u64::MAX));
        assert_eq!(row(&d.screen(), "Me "), format!("M ● Me {} {TADA}", you(lang)), "{w}x{h} {lang:?}");
    }
}

#[test]
fn narrow_rows_drop_the_status_then_you_then_clip_the_name() {
    // The DM rows' room for the name and what follows it: the panel's 28 columns but the chip
    // and mark (4), the gutter and one blank (2).
    const ROOM: usize = 22;
    for ((w, h), lang) in cases() {
        let named = |name: String| {
            move |s: &mut Snapshot| {
                let (c, me) = own_dm(s);
                c.name = Remote::new(name.clone());
                s.users.iter_mut().filter(|u| u.id == me).for_each(|u| u.display_name = Remote::new(name.clone()));
            }
        };
        let you = you(lang);
        let you_w = 1 + unicode_width::UnicodeWidthStr::width(you);
        // All of it fits: name, `you`, the status.
        let name = "N".repeat(ROOM - you_w - 3);
        let d = dms(w, h, lang, named(name.clone()));
        let at = format!("{w}x{h} {lang:?}:\n{}", d.screen());
        assert_eq!(row(&d.screen(), &name), format!("N ● {name} {you} {TADA}"), "{at}");
        // One cell more: the status goes.
        let name = "N".repeat(ROOM - you_w - 2);
        let d = dms(w, h, lang, named(name.clone()));
        assert_eq!(row(&d.screen(), &name), format!("N ● {name} {you}"), "{at}");
        // No room for `you` either: the name alone, whole.
        let name = "N".repeat(ROOM - 1);
        let d = dms(w, h, lang, named(name.clone()));
        assert_eq!(row(&d.screen(), &name), format!("N ● {name}"), "{at}");
        // Longer than the room: cut with `…`.
        let name = "N".repeat(ROOM + 5);
        let d = dms(w, h, lang, named(name));
        let cut = format!("{}…", "N".repeat(ROOM - 1));
        assert_eq!(row(&d.screen(), &cut), format!("N ● {cut}"), "{at}");
        // An app's tag stays right after the name and `you`, the status after it; short of
        // room the status and `you` give way, the tag never does.
        let app = d.app.i18n.label(slakio_core::i18n::Label::MessageApp).to_string();
        let tagged = |name: String| {
            move |s: &mut Snapshot| {
                named(name)(s);
                let (_, me) = own_dm(s);
                s.users.iter_mut().filter(|u| u.id == me).for_each(|u| u.bot = true);
            }
        };
        let d = dms(w, h, lang, tagged("Me".into()));
        assert_eq!(row(&d.screen(), "Me"), format!("M ● Me {you} {app} {TADA}"), "{at}");
        let app_w = unicode_width::UnicodeWidthStr::width(app.as_str());
        let name = "N".repeat(ROOM - 1 - app_w - you_w);
        let d = dms(w, h, lang, tagged(name.clone()));
        assert_eq!(row(&d.screen(), &name), format!("N ● {name} {you} {app}"), "{at}");
        let name = "N".repeat(ROOM + 5);
        let d = dms(w, h, lang, tagged(name));
        let cut = format!("{}…", "N".repeat(ROOM - 2 - app_w));
        assert_eq!(row(&d.screen(), &cut), format!("N ● {cut} {app}"), "{at}");
    }
}

/// Draw a frame and ask for what it wanted, as the binary's loop does: the lists and pictures
/// asked for (taken from those the backend holds).
fn frame(s: &mut Scripted) -> Vec<Request> {
    s.d.screen();
    s.d.app.ask_images(s.d.now);
    s.pump();
    let (asked, kept) = s.b.held.drain(..).partition(|r| matches!(r.command, Command::Image { .. } | Command::Emoji));
    s.b.held = kept;
    asked
}

/// The list answered as the world's backend does, then each picture asked for given.
fn drawn(s: &mut Scripted) {
    for _ in 0..4 {
        for r in frame(s) {
            if r.command == Command::Emoji {
                s.b.held.push(r);
                let i = s.b.held.len() - 1;
                s.b.answer(i);
                s.pump();
                continue;
            }
            let bytes = slakio_world::pictures::png("e", 64, 64);
            s.b.push(Envelope { generation: r.id, event: Event::Image { bytes } });
            s.pump();
            for effect in s.d.app.take_effects() {
                if let Effect::Decode(job) = effect {
                    let result = prepare(&job, Some((9, 18)));
                    s.d.app.images.decoded((job.source, job.request), result);
                }
            }
        }
    }
}

/// The placeholder cells on row `y`, with their foreground.
fn pictures_on(s: &Scripted, y: u16) -> Vec<(u16, ratatui::style::Color)> {
    let buf = s.d.buffer();
    (0..buf.area.width).filter(|&x| is_placeholder(buf[(x, y)].symbol())).map(|x| (x, buf[(x, y)].fg)).collect()
}

#[test]
fn a_custom_status_is_a_picture_whose_cells_keep_their_id() {
    for (w, h) in SIZES {
        let mut s = Scripted::booted(true, None);
        s.d.app.images.env = slakio_tui::images::detect::Env { term: "xterm-kitty".into(), ..Default::default() };
        (s.d.app.images.setting, s.d.app.images.tty) = ("auto".into(), true);
        s.d.app.images.resolve();
        assert!(s.d.app.images.on);
        s.d.app.theme = resolve("dark", true, Background::Dark);
        s.d.app.resize(w, h);
        s.keys("space d");
        drawn(&mut s);
        let ws = s.d.app.model.workspaces()[0].id.clone();
        let address = slakio_world::emoji::picture(ws.as_str(), "party-blob");
        let Look::Ready { id, .. } = s.d.app.images.look_emoji(&ws, &address) else { panic!("the picture is held") };
        let screen = s.d.screen();
        let at = format!("{w}x{h}:\n{screen}");
        assert!(!screen.contains(":party-blob:"), "{at}");
        let y = screen.lines().position(|l| l.contains("Jiho Park")).expect("Jiho's row") as u16;
        let x = cell_of(&s.d, "Jiho Park").0;
        let want = vec![(x + 10, color(id)), (x + 11, color(id))];
        assert_eq!(pictures_on(&s, y), want, "two cells one blank after the name: {at}");
        // Selected, then hovered (the cursor elsewhere): the cells stay as they are.
        let plain = s.d.buffer()[(x, y)].style();
        s.d.list_cursor_on_view("Jiho Park");
        assert_ne!(s.d.buffer()[(x, y)].style(), plain, "the bar is on the row: {at}");
        assert_eq!(pictures_on(&s, y), want, "under the selection bar: {at}");
        s.d.list_cursor_on_view("Minsu Kim");
        let before = s.d.buffer()[(x, y)].style();
        s.d.mouse(MouseEventKind::Moved, x, y);
        assert_ne!(s.d.buffer()[(x, y)].style(), before, "the row is hovered: {at}");
        assert_eq!(pictures_on(&s, y), want, "hovered: {at}");
    }
    // Pictures off: the name as text.
    for ((w, h), lang) in cases() {
        let d = dms(w, h, lang, |_| {});
        assert_eq!(row(&d.screen(), "Jiho Park"), "JP● Jiho Park :party-blob:", "{w}x{h} {lang:?}");
    }
}
