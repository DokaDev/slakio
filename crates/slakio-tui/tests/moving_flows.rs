//! A moving picture (a GIF emoji) under a popup that covers the messages (settings, the help)
//! stops; once the popup closes, the first frame drawn starts it again and the loop wakes for
//! its next frame by itself, with no other key or event. The turns follow the binary's loop
//! (`run` and `frame` in main.rs): a key is handled, the backend's turn comes, then one frame is
//! drawn (the pictures' commands written first, then the frame, then `App::ask_images`); the
//! loop then sleeps until `App::deadline` and calls `App::on_tick`, which asks for a redraw.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, Rgba, RgbaImage};
use ratatui::crossterm::event::Event as TermEvent;
use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, Request};
use slakio_core::model::{Message, Ts};
use slakio_core::sanitize::Remote;
use slakio_tui::app::Effect;
use slakio_tui::images::prepare;
use slakio_tui::keymap::parse_keys;

/// A 64 × 64 GIF of three frames, each shown 100 ms.
fn gif() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        enc.set_repeat(Repeat::Infinite).unwrap();
        for i in 0..3u8 {
            let img = RgbaImage::from_pixel(64, 64, Rgba([i * 80, 255 - i * 80, 128, 255]));
            enc.encode_frame(Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(100, 1))).unwrap();
        }
    }
    out
}

/// One frame as the binary's `frame` draws it: the pictures' commands written first (what is
/// returned), the frame drawn, then what it showed noted and asked for.
fn frame(s: &mut Scripted) -> String {
    let written = s.d.app.images.take_graphics();
    s.d.buffer();
    s.d.app.ask_images(s.d.now);
    written
}

/// The backend's turn, as at the top of the loop: what was asked for handed over, a picture
/// answered with the GIF and decoded, as the binary's decodes do.
fn turn(s: &mut Scripted) {
    s.pump();
    let asked: Vec<Request> = s.b.held.drain(..).collect();
    for r in &asked {
        match r.command {
            Command::Emoji => {
                s.b.held.push(r.clone());
                let i = s.b.held.len() - 1;
                s.b.answer(i);
            }
            Command::Image { .. } => s.b.push(Envelope { generation: r.id, event: Event::Image { bytes: gif() } }),
            _ => s.b.held.push(r.clone()),
        }
        s.pump();
    }
    for effect in s.d.app.take_effects() {
        if let Effect::Decode(job) = effect {
            let result = prepare(&job, Some((9, 18)));
            s.d.app.images.decoded((job.source, job.request), result);
        }
    }
}

/// Booted in kitty, `deploys` open with one message showing a GIF emoji of the workspace's own,
/// fetched, decoded and moving.
fn moving() -> Scripted {
    let mut s = Scripted::booted(true, None);
    s.d.app.images.env = slakio_tui::images::detect::Env { term: "xterm-kitty".into(), ..Default::default() };
    (s.d.app.images.setting, s.d.app.images.tty) = ("auto".into(), true);
    s.d.app.images.resolve();
    s.d.list_cursor_on("deploys");
    s.keys("enter");
    let mut e = s.b.reply(0);
    let Event::History(page) = &mut e.event else { panic!("a page") };
    let author = page.messages[0].user.clone();
    page.messages = vec![Message {
        ts: Ts(1_767_603_600_000_000),
        user: author,
        text: Remote::new("ship it :party-blob: now"),
        thread: None,
        reactions: Vec::new(),
        edited: false,
        emoji_pictures: Vec::new(),
        system: None,
        files: Vec::new(),
    }];
    page.complete = true;
    s.b.push(e);
    for _ in 0..4 {
        turn(&mut s);
        frame(&mut s);
    }
    assert!(!s.d.screen().contains(":party-blob:"), "the picture is drawn");
    s
}

/// One key handled as the loop handles an input event: `true` when it asks for a redraw.
fn key(s: &mut Scripted, notation: &str) -> bool {
    let mut redraw = false;
    for k in parse_keys(notation).unwrap() {
        redraw |= s.d.app.handle_event(TermEvent::Key(k.to_event()), s.d.now);
    }
    redraw
}

/// `open` covers the messages: the moving picture stops; `esc` closes it, and the one frame
/// drawn then starts it again, and its next frame is sent when due with nothing else happening.
fn moves_again_after(open: &str) {
    let mut s = moving();
    assert!(s.d.app.images.frame_at().is_some(), "it moves");

    assert!(key(&mut s, open));
    turn(&mut s);
    frame(&mut s);
    assert!(s.d.app.images.covered(), "{open} covers the messages");
    assert_eq!(s.d.app.images.frame_at(), None, "under {open} it stops");
    assert_eq!(s.d.app.deadline(), None, "the loop sleeps");

    assert!(key(&mut s, "esc"), "closing it asks for a redraw");
    turn(&mut s);
    frame(&mut s);
    assert!(!s.d.app.images.covered());
    let at = s.d.app.images.frame_at().expect("the first frame after it closes starts it again");
    assert_eq!(s.d.app.deadline(), Some(at), "the loop wakes for its next frame");

    s.d.now = at;
    assert!(s.d.app.on_tick(at), "the tick asks for a redraw");
    turn(&mut s);
    let written = frame(&mut s);
    assert!(written.starts_with("\x1b_Ga=T,U=1,f=100"), "its next frame is sent: {written:?}");
    assert!(s.d.app.images.frame_at().is_some_and(|next| next > at), "and the one after is due");
}

#[test]
fn a_moving_picture_starts_again_on_the_first_frame_after_settings_close() {
    moves_again_after("space ,");
}

#[test]
fn a_moving_picture_starts_again_on_the_first_frame_after_the_help_closes() {
    moves_again_after("?");
}
