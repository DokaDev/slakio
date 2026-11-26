//! A 1:1 DM's pane title starts with its peer's profile photo (`avatars = "image"`, the kitty
//! graphics protocol) in the two cells of the initials chip: the photo the list's row holds, so
//! nothing more is fetched. Pictures or photos off, the photo not here yet, or a title too
//! narrow to hold it whole: the initials. Avatars off: no chip. The focus, the selection bar,
//! the hover and a dialog's dim leave its cells as they are (their colour says which picture),
//! and a pane showing something else draws none of them.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
mod scripted;

use ratatui::crossterm::event::MouseEventKind;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, Generation};
use slakio_core::images::{AVATAR_HOSTS, Source};
use slakio_core::model::ConversationKind;
use slakio_tui::app::Effect;
use slakio_tui::app::shell::View;
use slakio_tui::images::graphics::{color, is_placeholder};
use slakio_tui::images::{CHIP_PHOTO, Look, prepare};
use std::collections::BTreeSet;

/// Pictures on as the binary sets them in kitty, `avatars = "image"`, and every DM peer of the
/// shown workspace with a photo address.
fn photos_on(s: &mut Scripted) {
    s.d.app.images.env = slakio_tui::images::detect::Env { term: "xterm-kitty".into(), ..Default::default() };
    (s.d.app.images.setting, s.d.app.images.tty) = ("auto".into(), true);
    s.d.app.images.resolve();
    assert!(s.d.app.images.on);
    s.d.app.settings.photos = true;
    let ws = s.d.app.model.workspaces()[0].id.clone();
    let peers: Vec<_> = (0..s.d.app.model.conversations().len())
        .filter_map(|i| match &s.d.app.model.conversation(i).kind {
            ConversationKind::Dm { user } if s.d.app.model.conversation(i).workspace == ws => Some(user.clone()),
            _ => None,
        })
        .collect();
    for peer in peers {
        let mut user = s.d.app.model.user(&ws, &peer).cloned().expect("the peer");
        user.avatar = Some(format!("https://{}/avatars/{ws}/{peer}-72.png", AVATAR_HOSTS[0]));
        s.d.app.model.add_users(&ws, &[user]);
    }
}

/// Booted at `w`×`h`, pictures and photos on (`on`), the list showing DMs.
fn dms(w: u16, h: u16, on: bool) -> Scripted {
    let mut s = Scripted::booted(true, None);
    s.d.app.resize(w, h);
    if on {
        photos_on(&mut s);
    } else {
        s.d.app.settings.photos = true;
    }
    s.keys("space d");
    assert_eq!(s.d.app.view(), View::Dms);
    s
}

/// The picture requests held (asked, not answered).
fn asked(s: &Scripted) -> Vec<(Generation, Source)> {
    s.b.held
        .iter()
        .filter_map(|r| match &r.command {
            Command::Image { source, .. } => Some((r.id, source.clone())),
            _ => None,
        })
        .collect()
}

/// Each picture waiting to be decoded, decoded as the binary does.
fn decode_all(s: &mut Scripted) {
    for effect in s.d.app.take_effects() {
        if let Effect::Decode(job) = effect {
            let result = prepare(&job, Some((9, 18)));
            s.d.app.images.decoded((job.source, job.request), result);
        }
    }
}

/// Draw a frame and ask for what it wanted, as the binary's loop does.
fn frame(s: &mut Scripted) -> Vec<(Generation, Source)> {
    s.d.screen();
    s.d.app.ask_images(s.d.now);
    s.pump();
    decode_all(s);
    asked(s)
}

/// Frames until nothing more is asked, each picture answered with a photo; every source asked.
fn fill(s: &mut Scripted) -> Vec<Source> {
    let mut all = Vec::new();
    for _ in 0..30 {
        let now = frame(s);
        if now.is_empty() {
            return all;
        }
        for (id, source) in now {
            let i = s.b.held.iter().position(|r| r.id == id).unwrap();
            s.b.held.remove(i);
            s.b.push(Envelope {
                generation: id,
                event: Event::Image { bytes: slakio_world::pictures::png("p", 72, 72) },
            });
            s.pump();
            decode_all(s);
            all.push(source);
        }
    }
    panic!("still asking after 30 frames")
}

/// The first DM of the list (the cursor's row on `space d`): its peer's display name and photo
/// address.
fn first_dm(s: &Scripted) -> (String, String) {
    let app = &s.d.app;
    let rows = app.list_rows();
    let slakio_tui::app::model::Row::Conversation(i) = rows[app.list_cursor()] else { panic!("a DM row") };
    let c = app.model.conversation(i);
    let ConversationKind::Dm { user } = &c.kind else { panic!("a 1:1 DM under the cursor") };
    let u = app.model.user(&c.workspace, user).unwrap();
    (u.display_name.line().as_str().to_string(), u.avatar.clone().unwrap())
}

/// The picture id of the DM photo of `address` the terminal holds.
fn id_of(s: &Scripted, address: &str) -> u32 {
    let ws = s.d.app.model.workspaces()[0].id.clone();
    match s.d.app.images.look_as(&ws, &Source::Avatar(address.to_string()), CHIP_PHOTO) {
        Look::Ready { id, cols: 2, rows: 1 } => id,
        other => panic!("{address}: {other:?}"),
    }
}

/// The picture id a placeholder's colour says (`u32::MAX`: not a picture's colour).
fn id_in(fg: Color) -> u32 {
    match fg {
        Color::Rgb(r, g, b) => (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
        _ => u32::MAX,
    }
}

/// The one pane on screen.
fn pane(s: &Scripted) -> Rect {
    let panes = s.d.app.panes_on_screen();
    assert_eq!(panes.len(), 1, "one pane");
    panes[0].1
}

/// The pane's title row as text, from its left corner to its right one.
fn title(s: &Scripted) -> String {
    let (a, buf) = (pane(s), s.d.buffer());
    (a.x..a.right()).map(|x| buf[(x, a.y)].symbol().to_string()).collect()
}

/// The placeholder cells on the pane's title row: (x, the picture id its colour says).
fn header_cells(s: &Scripted) -> BTreeSet<(u16, u32)> {
    let (a, buf) = (pane(s), s.d.buffer());
    (a.x..a.right()).filter(|&x| is_placeholder(buf[(x, a.y)].symbol())).map(|x| (x, id_in(buf[(x, a.y)].fg))).collect()
}

/// The placeholder cells the list's row of the first DM holds: (y, the picture id).
fn list_photo(s: &Scripted) -> BTreeSet<(u16, u32)> {
    let buf = s.d.buffer();
    let list = s.d.app.areas().list.expect("the list");
    (list.top()..list.bottom())
        .flat_map(|y| (list.left()..list.right()).map(move |x| (x, y)))
        .filter(|&(x, y)| is_placeholder(buf[(x, y)].symbol()))
        .map(|(x, y)| (y, id_in(buf[(x, y)].fg)))
        .collect()
}

/// The first DM opened in the pane, its photos (the list's) delivered when `on`.
fn opened(w: u16, h: u16, on: bool) -> Scripted {
    let mut s = dms(w, h, on);
    if on {
        fill(&mut s);
    }
    s.keys("enter");
    s
}

/// The title holds the peer's photo in the chip's two cells after the workspace stripe, then
/// its name: the picture `id`.
fn shows_photo(s: &Scripted, name: &str, id: u32, what: &str) {
    let a = pane(s);
    let line = title(s);
    let at = line.chars().position(|c| c == '▌').unwrap_or_else(|| panic!("{what}: a stripe: {line}"));
    let x = a.x + at as u16 + 1;
    assert_eq!(header_cells(s), BTreeSet::from([(x, id), (x + 1, id)]), "{what}: {line}");
    let buf = s.d.buffer();
    assert_eq!(buf[(x, a.y)].fg, color(id), "{what}");
    let rest: String = (x + 2..a.right()).map(|x| buf[(x, a.y)].symbol().to_string()).collect();
    assert!(rest.starts_with(&format!(" @{name} ")), "{what}: {rest:?}");
}

#[test]
fn a_dm_header_shows_the_peer_photo_once_ready() {
    for (w, h) in [(120, 40), (80, 24)] {
        let mut s = dms(w, h, true);
        let (name, address) = first_dm(&s);
        fill(&mut s);
        let id = id_of(&s, &address);
        s.keys("enter");
        assert!(frame(&mut s).is_empty(), "{w}x{h}: the list's photo: nothing more asked");
        shows_photo(&s, &name, id, &format!("{w}x{h}"));
        // The same picture as the list's row.
        assert!(list_photo(&s).iter().any(|&(_, i)| i == id), "{w}x{h}: the list's row shows it too");
    }
}

#[test]
fn a_dm_header_shows_initials_while_pending_and_with_images_off() {
    for (w, h) in [(120, 40), (80, 24)] {
        // Pending: asked, not answered.
        let mut s = dms(w, h, true);
        let (name, address) = first_dm(&s);
        assert!(!frame(&mut s).is_empty(), "{w}x{h}: the list's photos asked");
        s.keys("enter");
        frame(&mut s);
        assert!(header_cells(&s).is_empty(), "{w}x{h}: pending: no picture cell");
        let initials = slakio_tui::avatar::initials(&name, String::new);
        assert!(title(&s).contains(&format!("▌{initials} @{name} ")), "{w}x{h}: pending: {}", title(&s));
        // Failed: the initials stay.
        let held = asked(&s).into_iter().find(|(_, src)| *src == Source::Avatar(address.clone()));
        let (request, _) = held.unwrap_or_else(|| panic!("{w}x{h}: {address} asked"));
        let i = s.b.held.iter().position(|r| r.id == request).unwrap();
        s.b.fail(i, slakio_core::backend::FailKind::Network);
        s.pump();
        frame(&mut s);
        assert!(header_cells(&s).is_empty(), "{w}x{h}: failed: no picture cell");
        assert!(title(&s).contains(&format!("▌{initials} @{name} ")), "{w}x{h}: failed: {}", title(&s));
        // Pictures off (photos asked for): the initials, nothing asked.
        let mut s = opened(w, h, false);
        assert!(frame(&mut s).is_empty(), "{w}x{h}: pictures off: nothing asked");
        assert!(header_cells(&s).is_empty());
        assert!(title(&s).contains(&format!("▌{initials} @{name} ")), "{w}x{h}: off: {}", title(&s));
        // Photos off with pictures on: the initials, nothing asked.
        let mut s = dms(w, h, true);
        s.d.app.settings.photos = false;
        s.keys("enter");
        assert!(frame(&mut s).is_empty(), "{w}x{h}: initials: nothing asked");
        assert!(title(&s).contains(&format!("▌{initials} @{name} ")), "{w}x{h}: initials: {}", title(&s));
        // Avatars off: no chip, as before.
        let mut s = opened(w, h, true);
        s.d.app.settings.avatars = false;
        assert!(header_cells(&s).is_empty(), "{w}x{h}: avatars off");
        assert!(title(&s).contains(&format!("▌@{name} ")), "{w}x{h}: avatars off: {}", title(&s));
    }
}

#[test]
fn focus_and_selection_leave_the_header_photo_cells_alone() {
    for (w, h) in [(120, 40), (80, 24)] {
        let mut s = dms(w, h, true);
        let (name, address) = first_dm(&s);
        fill(&mut s);
        let id = id_of(&s, &address);
        let red = Color::Rgb(0xff, 0, 0);
        s.d.app.theme.styles.selection = Some(Style::new().fg(red).bg(Color::Rgb(0, 0xff, 0)));
        s.d.app.theme.styles.cursor_line = Some(Style::new().fg(red).bg(Color::Rgb(0, 0, 0xff)));
        s.keys("enter");
        let what = format!("{w}x{h}");
        shows_photo(&s, &name, id, &format!("{what} opened"));
        // The keyboard to the list and back.
        s.keys("ctrl+h");
        shows_photo(&s, &name, id, &format!("{what} list focused"));
        s.keys("ctrl+l");
        shows_photo(&s, &name, id, &format!("{what} pane focused"));
        // The mouse over the photo and over the title.
        let a = pane(&s);
        for x in a.x..a.x + 12 {
            s.d.mouse(MouseEventKind::Moved, x, a.y);
            shows_photo(&s, &name, id, &format!("{what} hover at {x}"));
        }
        // A dialog over the screen: the cells it leaves keep their picture's colour.
        s.keys(":");
        let now = header_cells(&s);
        assert!(now.iter().all(|&(_, i)| i == id), "{what}: a dimmed photo cell names another picture: {now:?}");
        s.keys("esc");
        shows_photo(&s, &name, id, &format!("{what} dialog closed"));
    }
}

#[test]
fn switching_the_pane_leaves_no_photo_cell_in_the_header() {
    for (w, h) in [(120, 40), (80, 24)] {
        let mut s = dms(w, h, true);
        let (name, address) = first_dm(&s);
        fill(&mut s);
        let id = id_of(&s, &address);
        s.keys("enter");
        shows_photo(&s, &name, id, &format!("{w}x{h}"));
        s.d.open("backend");
        frame(&mut s);
        assert!(title(&s).contains("#backend"), "{w}x{h}: {}", title(&s));
        assert!(header_cells(&s).is_empty(), "{w}x{h}: a channel's title holds no picture: {}", title(&s));
    }
}

/// The narrowest panes the layout makes (the smallest screen, the list hidden, split in two;
/// see `slakio_tui::screen::PANE_MIN_WIDTH`) still hold the whole photo in both titles; the
/// fallback below that width is pinned by the work module's own test.
#[test]
fn the_narrowest_panes_titles_hold_the_whole_photo() {
    let mut s = dms(120, 40, true);
    let (_, address) = first_dm(&s);
    fill(&mut s);
    let id = id_of(&s, &address);
    s.keys("enter");
    s.keys("space e");
    assert!(s.d.app.areas().list.is_none(), "the list hidden");
    s.d.app.resize(slakio_tui::screen::MIN_WIDTH, 24);
    s.keys("space w v");
    frame(&mut s);
    let panes = s.d.app.panes_on_screen();
    assert_eq!(panes.len(), 2, "split in two");
    let buf = s.d.buffer();
    for (_, a) in panes {
        let line: String = (a.x..a.right()).map(|x| buf[(x, a.y)].symbol().to_string()).collect();
        let photo: Vec<u16> = (a.x..a.right()).filter(|&x| is_placeholder(buf[(x, a.y)].symbol())).collect();
        assert_eq!(photo.len(), 2, "width {}: the whole photo: {line}", a.width);
        assert_eq!(photo[1], photo[0] + 1, "width {}: side by side", a.width);
        assert!(photo.iter().all(|&x| buf[(x, a.y)].fg == color(id)), "width {}", a.width);
    }
}
