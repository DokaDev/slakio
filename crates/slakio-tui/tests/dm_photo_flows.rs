//! Profile photos in the list's DM rows (`avatars = "image"`, the kitty graphics protocol): a
//! DM's peer's photo takes the two cells of its initials chip, so the presence mark and the name
//! stay where they are. Only the rows drawn ask for their photos, in the pictures' one queue
//! (at most [`IN_FLIGHT_MAX`] at once with the timeline's); a row scrolled, folded or resized
//! away, or a list the layout leaves out when the focus moves, draws none of its cells. Group
//! DMs keep their chip; pictures off, the initials stay and nothing is asked. The selection bar
//! and a dialog's dim leave the photo's cells as they are (their colour says which picture).

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
mod scripted;

use ratatui::layout::Rect;
use ratatui::style::Color;
use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, Generation};
use slakio_core::images::{AVATAR_HOSTS, Source};
use slakio_core::model::ConversationKind;
use slakio_tui::app::Effect;
use slakio_tui::app::model::Row;
use slakio_tui::app::shell::View;
use slakio_tui::images::graphics::{color, is_placeholder};
use slakio_tui::images::{CHIP_PHOTO, IN_FLIGHT_MAX, Look, prepare};
use slakio_tui::screen;
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

/// Booted at `w`×`h`, the list showing DMs, pictures and photos on (`on`).
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

/// Draw a frame and ask for what it wanted, as the binary's loop does.
fn frame(s: &mut Scripted) -> Vec<(Generation, Source)> {
    s.d.screen();
    s.d.app.ask_images(s.d.now);
    s.pump();
    asked(s)
}

/// Answer picture request `id` with a photo and decode it as the binary does.
fn deliver(s: &mut Scripted, id: Generation) {
    let i = s.b.held.iter().position(|r| r.id == id).unwrap();
    s.b.held.remove(i);
    s.b.push(Envelope { generation: id, event: Event::Image { bytes: slakio_world::pictures::png("p", 72, 72) } });
    s.pump();
    for effect in s.d.app.take_effects() {
        if let Effect::Decode(job) = effect {
            let result = prepare(&job, Some((9, 18)));
            s.d.app.images.decoded((job.source, job.request), result);
        }
    }
}

/// Frames until nothing more is asked, every picture delivered; each frame's new requests must
/// be photos of DM rows on screen when they are asked. Every source asked.
fn fill(s: &mut Scripted) -> Vec<Source> {
    let mut all = Vec::new();
    for _ in 0..30 {
        let on_screen: BTreeSet<String> = expected(s).into_iter().map(|p| p.address).collect();
        let now = frame(s);
        if now.is_empty() {
            return all;
        }
        assert!(now.len() <= IN_FLIGHT_MAX);
        for (id, source) in now {
            if let Source::Avatar(a) = &source {
                assert!(on_screen.contains(a), "{a} asked for, but no DM row on screen shows it");
            }
            all.push(source);
            deliver(s, id);
        }
    }
    panic!("still asking after 30 frames")
}

/// A DM row's photo where the list draws it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Photo {
    x: u16,
    y: u16,
    address: String,
}

/// The list's rows area now (`None`: the list is left out).
fn rows_area(s: &Scripted) -> Option<Rect> {
    let list = s.d.app.areas().list?;
    Some(screen::list_parts(list, s.d.app.nav_rows()).rows)
}

/// The photos the list should show now: one at the chip of each DM row on screen whose peer has
/// a photo address.
fn expected(s: &Scripted) -> Vec<Photo> {
    let Some(rows) = rows_area(s) else { return Vec::new() };
    let app = &s.d.app;
    let x0 = if app.view() == View::Home { 3 } else { 1 };
    let top = app.list_top();
    let mut out = Vec::new();
    for (k, row) in app.list_rows().into_iter().enumerate().skip(top).take(usize::from(rows.height)) {
        let Row::Conversation(i) = row else { continue };
        let c = app.model.conversation(i);
        let ConversationKind::Dm { user } = &c.kind else { continue };
        let Some(address) = app.model.user(&c.workspace, user).and_then(|u| u.avatar.clone()) else { continue };
        out.push(Photo { x: rows.x + x0, y: rows.y + (k - top) as u16, address });
    }
    out
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
        Color::Rgb(r, g, b) => {
            let id = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
            assert_eq!(color(id), fg);
            id
        }
        _ => u32::MAX,
    }
}

/// Every placeholder cell on screen: (x, y, the picture id its colour says).
fn cells(s: &Scripted) -> BTreeSet<(u16, u16, u32)> {
    let buf = s.d.buffer();
    let a = buf.area;
    (a.top()..a.bottom())
        .flat_map(|y| (a.left()..a.right()).map(move |x| (x, y)))
        .filter(|&(x, y)| is_placeholder(buf[(x, y)].symbol()))
        .map(|(x, y)| (x, y, id_in(buf[(x, y)].fg)))
        .collect()
}

/// The placeholder cells the list's DM rows should hold now, each photo two cells on one row.
fn want(s: &Scripted) -> BTreeSet<(u16, u16, u32)> {
    expected(s)
        .into_iter()
        .flat_map(|p| {
            let c = id_of(s, &p.address);
            [(p.x, p.y, c), (p.x + 1, p.y, c)]
        })
        .collect()
}

/// The screen's placeholders are exactly the DM photos of the rows on screen.
fn exactly_the_rows_on_screen(s: &Scripted, what: &str) {
    let want = want(s);
    assert!(
        !want.is_empty() || rows_area(s).is_none(),
        "{what}: some DM row on screen (top {}, view {:?}, rows {:?}):\n{}",
        s.d.app.list_top(),
        s.d.app.view(),
        s.d.app.list_rows().len(),
        s.d.screen()
    );
    assert_eq!(cells(s), want, "{what}:\n{}", s.d.screen());
}

/// Row `y` of the list from column `x` to its right edge, as text.
fn rest_of_row(s: &Scripted, x: u16, y: u16) -> String {
    let buf = s.d.buffer();
    let right = s.d.app.areas().list.map_or(x, |l| l.right());
    (x..right).map(|x| buf[(x, y)].symbol().to_string()).collect()
}

#[test]
fn dm_rows_on_screen_show_their_peers_photos_in_the_chip_s_cells() {
    for (w, h) in [(120, 40), (80, 24)] {
        let mut s = dms(w, h, true);
        assert!(cells(&s).is_empty(), "{w}x{h}: initials until the photos come");
        let asked = fill(&mut s);
        let shown: BTreeSet<String> = expected(&s).into_iter().map(|p| p.address).collect();
        let asked: BTreeSet<String> = asked.iter().map(|a| a.address().to_string()).collect();
        assert_eq!(asked, shown, "{w}x{h}: the photos of the rows on screen, each once");
        exactly_the_rows_on_screen(&s, &format!("{w}x{h}"));
        assert!(s.d.app.images.take_graphics().contains("c=2,r=1"), "{w}x{h}: sent for 2 × 1 cells");
        // Nothing else moves: the presence mark and the name are where they are with initials.
        let off = dms(w, h, false);
        assert_eq!(off.d.app.list_top(), s.d.app.list_top());
        for p in expected(&s) {
            assert_eq!(rest_of_row(&s, p.x + 2, p.y), rest_of_row(&off, p.x + 2, p.y), "{w}x{h}: row {}", p.y);
            assert!(!is_placeholder(off.d.buffer()[(p.x, p.y)].symbol()), "{w}x{h}: initials with pictures off");
        }
    }
}

#[test]
fn rows_off_screen_ask_for_nothing_and_a_scroll_moves_the_photos_with_their_rows() {
    let mut s = dms(80, 16, true);
    let dm_rows = s.d.app.list_rows().len();
    let height = usize::from(rows_area(&s).unwrap().height);
    assert!(dm_rows > height, "more DMs ({dm_rows}) than rows ({height})");
    let first = fill(&mut s);
    let below: Vec<String> = {
        let app = &s.d.app;
        let ws = app.model.workspaces()[0].id.clone();
        app.list_rows()
            .into_iter()
            .skip(height)
            .filter_map(|r| match r {
                Row::Conversation(i) => match &app.model.conversation(i).kind {
                    ConversationKind::Dm { user } => app.model.user(&ws, user).and_then(|u| u.avatar.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    };
    assert!(!below.is_empty());
    assert!(first.iter().all(|a| !below.contains(&a.address().to_string())), "rows below the list asked for nothing");
    let top_photo = expected(&s)[0].clone();
    let top_id = id_of(&s, &top_photo.address);
    // Down past the bottom: the list scrolls.
    for _ in 0..dm_rows {
        s.keys("j");
    }
    assert!(s.d.app.list_top() > 0, "scrolled");
    fill(&mut s);
    exactly_the_rows_on_screen(&s, "scrolled");
    if !expected(&s).iter().any(|p| p.address == top_photo.address) {
        assert!(cells(&s).iter().all(|c| c.2 != top_id), "the row scrolled away left nothing behind");
    }
    // Back up: the photos held are drawn again, nothing asked again.
    s.keys("g g");
    assert_eq!(s.d.app.list_top(), 0);
    assert!(frame(&mut s).is_empty(), "held: not asked again");
    exactly_the_rows_on_screen(&s, "back at the top");
}

#[test]
fn a_group_dm_keeps_its_chip() {
    let mut s = dms(120, 40, true);
    fill(&mut s);
    let rows = rows_area(&s).unwrap();
    let app = &s.d.app;
    let groups: Vec<u16> = app
        .list_rows()
        .into_iter()
        .enumerate()
        .take(usize::from(rows.height))
        .filter_map(|(k, r)| match r {
            Row::Conversation(i) if matches!(app.model.conversation(i).kind, ConversationKind::GroupDm { .. }) => {
                Some(rows.y + k as u16)
            }
            _ => None,
        })
        .collect();
    assert!(!groups.is_empty(), "a group DM on screen");
    let all = cells(&s);
    for y in groups {
        assert!(all.iter().all(|c| c.1 != y), "row {y}: no picture");
        let chip = rest_of_row(&s, rows.x + 1, y);
        assert!(chip.starts_with(|c: char| c.is_ascii_digit()), "row {y}: how many people: {chip}");
    }
    exactly_the_rows_on_screen(&s, "groups");
}

#[test]
fn pictures_or_photos_off_keep_the_initials_and_ask_for_nothing() {
    let mut s = dms(120, 40, false);
    let initials = s.d.screen();
    assert!(frame(&mut s).is_empty(), "pictures off: nothing asked");
    assert!(cells(&s).is_empty(), "pictures off: no picture cell");
    // Pictures on, avatars as initials: still the initials, nothing asked.
    photos_on(&mut s);
    s.d.app.settings.photos = false;
    assert!(frame(&mut s).is_empty(), "initials: nothing asked");
    assert_eq!(s.d.screen(), initials, "initials: the list as it was");
    // Photos: they come in the chips' cells.
    s.d.app.settings.photos = true;
    fill(&mut s);
    exactly_the_rows_on_screen(&s, "photos");
    assert_eq!(s.d.app.images.take_graphics().matches("a=T").count(), expected(&s).len());
}

#[test]
fn images_off_removes_every_dm_photo_and_brings_the_initials_back() {
    let mut s = dms(120, 40, true);
    fill(&mut s);
    let ids: Vec<u32> = expected(&s).iter().map(|p| id_of(&s, &p.address)).collect();
    assert!(!ids.is_empty());
    s.d.app.images.take_graphics();
    use ratatui::crossterm::event::{Event as Term, KeyCode, KeyEvent, KeyModifiers};
    s.keys(":");
    for c in "images off".chars() {
        s.d.app.handle_event(Term::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)), s.d.now);
    }
    s.keys("enter");
    assert!(!s.d.app.images.on);
    let sent = s.d.app.images.take_graphics();
    for id in &ids {
        assert!(sent.contains(&format!("a=d,d=I,i={id},q=2")), "photo {id} removed from the terminal: {sent:?}");
    }
    assert!(sent.ends_with("\x1b_Ga=d,d=A,q=2\x1b\\"));
    assert!(cells(&s).is_empty(), "no picture cell left");
    let off = dms(120, 40, false);
    for p in expected(&s) {
        assert_eq!(rest_of_row(&s, p.x, p.y), rest_of_row(&off, p.x, p.y), "row {}: the initials again", p.y);
    }
    assert!(frame(&mut s).is_empty(), "nothing asked");
}

#[test]
fn a_folded_section_or_another_view_leaves_no_photo_behind() {
    let mut s = dms(120, 40, true);
    fill(&mut s);
    assert!(!cells(&s).is_empty());
    // Home, unfolded, the cursor on the first DM: its section shows its rows with their photos.
    let first = (0..s.d.app.model.conversations().len())
        .find(|i| matches!(s.d.app.model.conversation(*i).kind, ConversationKind::Dm { .. }))
        .unwrap();
    let name = s.d.app.model.conversation(first).name.line().as_str().to_string();
    s.d.list_cursor_on(&name);
    // A step down and back: the list scrolls to the cursor.
    s.keys("j k");
    fill(&mut s);
    exactly_the_rows_on_screen(&s, "home");
    // Fold the section of the first DM on screen.
    let rows = s.d.app.list_rows();
    let first_dm = rows
        .iter()
        .position(|r| matches!(r, Row::Conversation(i) if matches!(s.d.app.model.conversation(*i).kind, ConversationKind::Dm { .. })))
        .unwrap();
    let section = rows[..first_dm].iter().rposition(|r| matches!(r, Row::Section(_))).unwrap();
    let before = expected(&s).len();
    s.d.app.focus_list_row(section);
    s.keys("enter");
    assert!(expected(&s).len() < before, "the folded section's DMs are gone");
    fill(&mut s);
    assert_eq!(cells(&s), want(&s), "folded:\n{}", s.d.screen());
    // To-dos lists no conversation: no photo.
    s.d.app.show_unfolded(View::Todos);
    assert_eq!(s.d.app.view(), View::Todos);
    assert!(cells(&s).is_empty(), "another view: no picture cell:\n{}", s.d.screen());
}

#[test]
fn a_resize_draws_the_photos_of_the_rows_it_leaves_and_nothing_else() {
    let mut s = dms(120, 40, true);
    fill(&mut s);
    exactly_the_rows_on_screen(&s, "120x40");
    let tall = expected(&s).len();
    s.d.app.resize(80, 16);
    let short = expected(&s).len();
    assert!(short < tall, "fewer rows ({short} < {tall})");
    fill(&mut s);
    exactly_the_rows_on_screen(&s, "80x16");
    s.d.app.resize(40, 9);
    assert!(cells(&s).is_empty(), "too small: no picture cell");
    s.d.app.resize(120, 40);
    assert!(frame(&mut s).is_empty(), "held: drawn again, not asked");
    exactly_the_rows_on_screen(&s, "120x40 again");
}

/// Answer every request held but the pictures, until none is left.
fn settle(s: &mut Scripted) {
    for _ in 0..20 {
        let Some(i) = s.b.held.iter().position(|r| !matches!(r.command, Command::Image { .. })) else { return };
        s.b.answer(i);
        s.pump();
    }
}

#[test]
fn the_list_left_out_when_the_focus_moves_leaves_no_photo_behind() {
    let mut s = Scripted::booted(true, None);
    s.d.app.resize(80, 24);
    photos_on(&mut s);
    s.d.list_cursor_on("long-threads");
    s.keys("enter");
    settle(&mut s);
    s.keys("g g enter");
    settle(&mut s);
    assert!(s.d.app.areas().list.is_none(), "the thread panel took the list's room");
    s.keys("space d");
    settle(&mut s);
    assert!(s.d.app.areas().list.is_some(), "the list is back with the focus");
    fill(&mut s);
    exactly_the_rows_on_screen(&s, "list focused");
    let photos: BTreeSet<u32> = want(&s).into_iter().map(|c| c.2).collect();
    assert!(!photos.is_empty());
    s.keys("ctrl+l");
    assert!(s.d.app.areas().list.is_none(), "the focus left: the list is left out");
    let left: Vec<_> = cells(&s).into_iter().filter(|c| photos.contains(&c.2)).collect();
    assert!(left.is_empty(), "no DM photo left on screen: {left:?}\n{}", s.d.screen());
    // Back to the list: drawn again from what the terminal holds, nothing asked.
    s.keys("space d");
    assert!(frame(&mut s).is_empty());
    exactly_the_rows_on_screen(&s, "list again");
}

#[test]
fn the_list_s_photos_wait_in_the_pictures_one_queue() {
    let mut s = Scripted::booted(true, None);
    s.d.app.resize(120, 40);
    photos_on(&mut s);
    s.d.list_cursor_on("deploys");
    s.keys("enter");
    settle(&mut s);
    s.keys("space d");
    let list = expected(&s);
    assert!(list.len() > IN_FLIGHT_MAX, "more DM photos on screen ({}) than may be asked at once", list.len());
    let first = frame(&mut s);
    assert_eq!(first.len(), IN_FLIGHT_MAX, "the list and the timeline share {IN_FLIGHT_MAX}: {first:?}");
    assert_eq!(frame(&mut s).len(), IN_FLIGHT_MAX, "none more while they are on their way");
    // One fails: one more may be asked, never more than the bound.
    let i = s.b.held.iter().position(|r| r.id == first[0].0).unwrap();
    s.b.fail(i, slakio_core::backend::FailKind::Network);
    s.pump();
    assert_eq!(frame(&mut s).len(), IN_FLIGHT_MAX);
    // Everything delivered as it comes, the bound held at every frame: the list's photos came
    // through the same queue as the timeline's.
    for _ in 0..40 {
        let held = frame(&mut s);
        assert!(held.len() <= IN_FLIGHT_MAX, "{} at once", held.len());
        if held.is_empty() {
            break;
        }
        for (id, _) in held {
            deliver(&mut s, id);
        }
    }
    let ws = s.d.app.model.workspaces()[0].id.clone();
    for p in &list {
        let look = s.d.app.images.look_as(&ws, &Source::Avatar(p.address.clone()), CHIP_PHOTO);
        // The one that failed waits for its retry.
        let failed = first[0].1 == Source::Avatar(p.address.clone());
        assert!(matches!(look, Look::Ready { .. }) || failed, "{}: {look:?}", p.address);
    }
}

#[test]
fn the_selection_bar_and_its_styles_leave_the_photo_s_cells_as_they_are() {
    use ratatui::style::Style;
    let mut s = dms(120, 40, true);
    fill(&mut s);
    let red = Color::Rgb(0xff, 0, 0);
    s.d.app.theme.styles.selection = Some(Style::new().fg(red).bg(Color::Rgb(0, 0xff, 0)));
    s.d.app.theme.styles.cursor_line = Some(Style::new().fg(red).bg(Color::Rgb(0, 0, 0xff)));
    let first = expected(&s)[0].clone();
    let at = s.d.app.list_rows().iter().position(|r| matches!(r, Row::Conversation(_))).unwrap();
    s.d.app.focus_list_row(at);
    s.pump();
    for how in ["focused", "unfocused"] {
        let row = rows_area(&s).unwrap().y + (at - s.d.app.list_top()) as u16;
        assert_eq!(row, first.y, "{how}: the cursor is on the first DM");
        let buf = s.d.buffer();
        assert_eq!(buf[(first.x + 4, row)].fg, red, "{how}: the bar's style is on the name");
        exactly_the_rows_on_screen(&s, how);
        s.keys("ctrl+l");
    }
}

#[test]
fn a_dialog_s_dim_keeps_every_dm_photo_s_colour_its_id() {
    for theme in ["high-contrast", "tokyo-night", "dark", "light"] {
        let mut s = dms(120, 40, true);
        s.d.app.set_theme(theme, s.d.now).unwrap();
        fill(&mut s);
        let want = want(&s);
        s.keys(":");
        let now = cells(&s);
        assert!(!now.is_empty(), "{theme}: some photos around the palette");
        assert!(now.is_subset(&want), "{theme}: a dimmed photo cell names another picture: {now:?} vs {want:?}");
        s.keys("esc");
        assert_eq!(cells(&s), want, "{theme}: all back");
    }
}
