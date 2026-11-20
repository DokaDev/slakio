//! Pictures in the timeline (the kitty graphics protocol): a picture a message shares is asked
//! for once it would be drawn, decoded, sent to the terminal once and drawn as placeholder
//! cells under its card; what is drawn over it (a popup) replaces its cells, so nothing of it is
//! left there; a pane too narrow for it shows the card alone; one that failed says why on its
//! card. With `avatars = "image"`, a sender's photo fills the comfortable picture slot; the
//! initials stay until it comes and where it does not. Pictures off (the default here, as in
//! every other test), nothing of this is drawn or asked.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
mod scripted;

use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, FailKind, Generation};
use slakio_core::images::Source;
use slakio_core::model::{FILE_HOST, File, FileId, Message, Ts, UserId};
use slakio_core::sanitize::Remote;
use slakio_tui::app::Effect;
use slakio_tui::images::decode::decode;
use slakio_tui::images::graphics::is_placeholder;

const THUMB: &str = "files-tmb/T-F1/shot_360.png";

fn picture_file() -> File {
    File {
        id: FileId::new("F1"),
        name: Remote::new("shot.png"),
        kind: Remote::new("PNG"),
        size: Some(345_678),
        download: Some(format!("https://{FILE_HOST}/files-pri/T-F1/download/shot.png")),
        thumb: Some(format!("https://{FILE_HOST}/{THUMB}")),
    }
}

/// Booted, `deploys` open at `w`×`h`, its one message sharing a picture, pictures `on`.
fn shown(w: u16, h: u16, on: bool) -> (Scripted, UserId) {
    let mut s = Scripted::booted(true, None);
    s.d.app.images.on = on;
    s.d.app.resize(w, h);
    s.d.list_cursor_on("deploys");
    s.keys("enter");
    let mut e = s.b.reply(0);
    let Event::History(page) = &mut e.event else { panic!("a page") };
    let author = page.messages[0].user.clone();
    page.messages = vec![Message {
        ts: Ts(1_767_603_600_000_000),
        user: author.clone(),
        text: Remote::new("look"),
        thread: None,
        reactions: Vec::new(),
        edited: false,
        files: vec![picture_file()],
    }];
    page.complete = true;
    s.b.push(e);
    s.pump();
    (s, author)
}

/// Draw a frame and ask for what it wanted, as the binary's loop does: the picture requests.
fn frame(s: &mut Scripted) -> Vec<(Generation, Source)> {
    s.d.screen();
    s.d.app.ask_images(s.d.now);
    s.pump();
    s.b.held
        .iter()
        .filter_map(|r| match &r.command {
            Command::Image { source, .. } => Some((r.id, source.clone())),
            _ => None,
        })
        .collect()
}

/// Answer picture request `id` with `bytes` and decode it as the binary does.
fn deliver(s: &mut Scripted, id: Generation, bytes: Vec<u8>) {
    let i = s.b.held.iter().position(|r| r.id == id).unwrap();
    s.b.held.remove(i);
    s.b.push(Envelope { generation: id, event: Event::Image { bytes } });
    s.pump();
    for effect in s.d.app.take_effects() {
        if let Effect::Decode(job) = effect {
            let result = decode(&job.bytes, job.shape, Some((9, 18)));
            s.d.app.images.decoded(job.source, result);
        }
    }
}

fn placeholders(s: &Scripted) -> usize {
    let buf = s.d.buffer();
    buf.content().iter().filter(|c| is_placeholder(c.symbol())).count()
}

#[test]
fn off_nothing_is_asked_and_the_card_is_as_before() {
    let (mut s, _) = shown(120, 40, false);
    assert!(frame(&mut s).is_empty());
    assert_eq!(placeholders(&s), 0);
    assert!(s.d.screen().contains("shot.png  PNG · 346 KB"), "{}", s.d.screen());
}

#[test]
fn a_picture_comes_once_and_is_drawn_under_its_card() {
    let (mut s, _) = shown(120, 40, true);
    let asked = frame(&mut s);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].1, Source::Thumb(format!("https://{FILE_HOST}/{THUMB}")));
    assert!(frame(&mut s).len() == 1, "drawn again while it comes: not asked again");
    assert_eq!(placeholders(&s), 0, "nothing before it came");
    deliver(&mut s, asked[0].0, slakio_world::pictures::png("x", 360, 240));
    let sent = s.d.app.images.take_graphics();
    assert!(sent.starts_with("\x1b_Ga=T,U=1,f=100"), "sent to the terminal");
    let screen = s.d.screen();
    let card = screen.lines().position(|l| l.contains("shot.png")).unwrap();
    let buf = s.d.buffer();
    let row = |y: usize| (0..120).filter(|&x| is_placeholder(buf[(x, y as u16)].symbol())).count();
    assert_eq!(row(card), 0, "the card stays");
    assert_eq!(row(card + 1), 30, "30 × 10 cells under it");
    assert_eq!(placeholders(&s), 300);
    assert!(frame(&mut s).is_empty(), "never asked again");
    assert_eq!(s.d.app.images.take_graphics(), "", "sent once");
}

#[test]
fn a_popup_over_a_picture_replaces_its_cells() {
    let (mut s, _) = shown(120, 40, true);
    let asked = frame(&mut s);
    deliver(&mut s, asked[0].0, slakio_world::pictures::png("x", 360, 240));
    let before = placeholders(&s);
    assert!(before > 0);
    // The keys' help covers most of the screen.
    s.keys("?");
    assert!(s.d.screen().contains("Keys —"), "the help is up:\n{}", s.d.screen());
    assert!(placeholders(&s) < before, "covered cells hold no picture:\n{}", s.d.screen());
    s.keys("esc");
    assert_eq!(placeholders(&s), before, "back once it is gone");
}

#[test]
fn a_pane_too_narrow_shows_the_card_alone() {
    let (mut s, _) = shown(120, 40, true);
    let asked = frame(&mut s);
    deliver(&mut s, asked[0].0, slakio_world::pictures::png("x", 360, 240));
    s.d.app.resize(64, 24);
    assert_eq!(placeholders(&s), 0, "{}", s.d.screen());
    assert!(s.d.screen().contains(".png"), "{}", s.d.screen());
}

#[test]
fn a_picture_that_failed_says_why_on_its_card() {
    for (kind, words) in [
        (FailKind::TooLarge, "too large to preview"),
        (FailKind::Api { code: "not_an_image".into() }, "no preview"),
        (FailKind::Network, "preview not loaded"),
    ] {
        let (mut s, _) = shown(120, 40, true);
        let asked = frame(&mut s);
        let i = s.b.held.iter().position(|r| r.id == asked[0].0).unwrap();
        s.b.fail(i, kind.clone());
        s.pump();
        let screen = s.d.screen();
        let card = screen.lines().find(|l| l.contains("shot.png")).unwrap();
        assert!(card.contains(words), "{kind:?}: {card}");
        assert!(!s.d.status_line().contains(words), "{kind:?}: not in the status line");
    }
    // Bytes that are no picture: refused, said, never a panic.
    let (mut s, _) = shown(120, 40, true);
    let asked = frame(&mut s);
    deliver(&mut s, asked[0].0, b"<html>sign in</html>".to_vec());
    assert!(s.d.screen().contains("no preview"));
    assert_eq!(s.d.app.images.take_graphics(), "");
}

#[test]
fn a_sender_s_photo_fills_the_picture_slot_with_avatars_image() {
    let (mut s, author) = shown(120, 40, true);
    s.d.app.settings.photos = true;
    let photo = format!("https://{}/avatars/T/{}-72.png", slakio_core::images::AVATAR_HOSTS[0], author);
    let mut user = s.d.app.model.user(&s.d.app.model.workspaces()[0].id, &author).cloned().expect("the author");
    user.avatar = Some(photo.clone());
    s.d.app.model.add_users(&user.workspace.clone(), &[user]);
    let asked = frame(&mut s);
    let a = asked.iter().find(|(_, src)| *src == Source::Avatar(photo.clone())).expect("the photo is asked for");
    let screen = s.d.screen();
    assert!(!screen.contains('\u{10EEEE}'));
    deliver(&mut s, a.0, slakio_world::pictures::png("p", 72, 72));
    let buf = s.d.buffer();
    let at: Vec<(u16, u16)> = (0..40u16)
        .flat_map(|y| (0..120u16).map(move |x| (x, y)))
        .filter(|&(x, y)| is_placeholder(buf[(x, y)].symbol()))
        .collect();
    assert!(at.len() >= 8, "a 4 × 2 photo: {at:?}");
    // Initials stay where photos are not on.
    s.d.app.settings.photos = false;
    let before = placeholders(&s);
    assert!(before < at.len(), "the photo's cells are gone with photos off");
}

/// Type `:text` and `Enter` in the command palette.
fn palette(s: &mut Scripted, text: &str) {
    use ratatui::crossterm::event::{Event as Term, KeyCode, KeyEvent, KeyModifiers};
    s.keys(":");
    for c in text.chars() {
        s.d.app.handle_event(Term::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)), s.d.now);
    }
    s.keys("enter");
}

/// [`shown`] with what decides pictures as the binary sets it: kitty, setting `auto`.
fn kitty_shown() -> Scripted {
    let (mut s, _) = shown(120, 40, false);
    s.d.app.images.env = slakio_tui::images::detect::Env { term: "xterm-kitty".into(), ..Default::default() };
    (s.d.app.images.setting, s.d.app.images.tty) = ("auto".into(), true);
    s.d.app.images.resolve();
    assert!(s.d.app.images.on);
    s
}

fn saved(s: &mut Scripted) -> Vec<(String, String)> {
    s.d.app
        .take_effects()
        .into_iter()
        .filter_map(|e| match e {
            Effect::Save { key, value } => Some((key.to_string(), value)),
            _ => None,
        })
        .collect()
}

#[test]
fn images_off_clears_every_picture_at_once_and_sends_nothing_more() {
    let mut s = kitty_shown();
    let asked = frame(&mut s);
    deliver(&mut s, asked[0].0, slakio_world::pictures::png("x", 360, 240));
    s.d.app.images.take_graphics();
    assert!(placeholders(&s) > 0);
    palette(&mut s, "images off");
    assert!(!s.d.app.images.on);
    assert_eq!(saved(&mut s), [("images".to_string(), "off".to_string())], "kept in the config file");
    assert_eq!(s.d.app.images.take_graphics(), "\x1b_Ga=d,d=A,q=2\x1b\\", "every picture removed at once");
    assert_eq!(placeholders(&s), 0, "text cards again");
    assert!(s.d.screen().contains("shot.png  PNG · 346 KB"));
    assert!(s.d.status_line().contains("Images: off"), "{}", s.d.status_line());
    // Frames after it ask for nothing and send nothing.
    for _ in 0..3 {
        assert!(frame(&mut s).len() <= 1, "nothing new asked");
        assert_eq!(s.d.app.images.take_graphics(), "");
    }
    assert!(!s.d.app.ask_images(s.d.now));
}

#[test]
fn images_on_again_draws_what_is_visible_and_alone_it_switches() {
    let mut s = kitty_shown();
    palette(&mut s, "images off");
    s.b.held.retain(|r| !matches!(r.command, Command::Image { .. }));
    palette(&mut s, "images auto");
    assert!(s.d.app.images.on);
    assert_eq!(saved(&mut s).last().map(|(_, v)| v.as_str()), Some("auto"));
    let asked = frame(&mut s);
    assert_eq!(asked.len(), 1, "what is visible is asked for again");
    deliver(&mut s, asked[0].0, slakio_world::pictures::png("x", 360, 240));
    assert!(placeholders(&s) > 0);
    // `:images` alone: off, then back to auto.
    palette(&mut s, "images");
    assert!(!s.d.app.images.on);
    palette(&mut s, "images");
    assert!(s.d.app.images.on);
    assert_eq!(s.d.app.images.setting, "auto");
}

#[test]
fn images_on_where_it_cannot_tell_says_so_and_the_variable_says_it_decides() {
    let (mut s, _) = shown(120, 40, false);
    s.d.app.images.env = slakio_tui::images::detect::Env { term: "xterm-256color".into(), ..Default::default() };
    (s.d.app.images.setting, s.d.app.images.tty) = ("auto".into(), true);
    s.d.app.images.resolve();
    palette(&mut s, "images on");
    assert!(s.d.app.images.on, "the user's choice");
    assert!(s.d.status_line().contains("Images forced on"), "{}", s.d.status_line());
    let mut pinned = kitty_shown();
    pinned.d.app.images.env.images = Some("off".into());
    pinned.d.app.images.resolve();
    palette(&mut pinned, "images on");
    assert!(!pinned.d.app.images.on, "the variable wins");
    assert!(pinned.d.status_line().contains("SLAKIO_IMAGES=off"), "{}", pinned.d.status_line());
    assert_eq!(saved(&mut pinned).last().map(|(_, v)| v.as_str()), Some("on"), "kept for a run without it");
    palette(&mut pinned, "images sometimes");
    let screen = pinned.d.screen();
    assert!(screen.contains("Unknown images setting sometimes") && screen.contains("auto, on, off"), "{screen}");
}

#[test]
fn leaving_avatars_image_removes_the_photos() {
    let (mut s, author) = shown(120, 40, true);
    palette(&mut s, "avatars image");
    assert!(s.d.app.settings.photos);
    let photo = format!("https://{}/avatars/T/{}-72.png", slakio_core::images::AVATAR_HOSTS[0], author);
    let mut user = s.d.app.model.user(&s.d.app.model.workspaces()[0].id, &author).cloned().unwrap();
    user.avatar = Some(photo.clone());
    s.d.app.model.add_users(&user.workspace.clone(), &[user]);
    let asked = frame(&mut s);
    let a = asked.iter().find(|(_, src)| *src == Source::Avatar(photo.clone())).unwrap().0;
    deliver(&mut s, a, slakio_world::pictures::png("p", 72, 72));
    s.d.app.images.take_graphics();
    palette(&mut s, "avatars initials");
    assert!(s.d.app.images.take_graphics().contains("a=d,d=I,"), "the photo leaves the terminal");
    assert_eq!(placeholders(&s), 0, "initials again");
}
