//! The first load of a busy channel, over a scripted backend that answers each picture at once:
//! 20 people's photos and 40 of the workspace's own emoji on one screen. The timeline's pictures
//! are asked before the list's photos, in the order the timeline draws them (the newest message
//! first, each message's emoji left to right, then its author's photo), at most
//! [`CDN_IN_FLIGHT_MAX`] at once and the next as soon as one is in: no wait of its own between asks,
//! so the screen fills within a frame per twelve pictures. A frame draws at most
//! [`EMOJI_PER_FRAME`] distinct emoji (the rest stay `:name:`). A picture that fails for the
//! budget wakes the loop when it may be asked again, with no key pressed.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
mod scripted;

use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, FailKind, Request};
use slakio_core::images::emoji::Custom;
use slakio_core::images::{AVATAR_HOSTS, Source};
use slakio_core::model::{Message, Org, Presence, Ts, User, UserId, WorkspaceId};
use slakio_core::sanitize::Remote;
use slakio_tui::app::Effect;
use slakio_tui::images::decode::Shape;
use slakio_tui::images::{CDN_IN_FLIGHT_MAX, EMOJI_PER_FRAME, Look, prepare};

const PEOPLE: usize = 20;
const EMOJI: usize = 40;

fn photo(ws: &WorkspaceId, i: usize) -> String {
    format!("https://{}/avatars/{ws}/UP{i:02}-72.png", AVATAR_HOSTS[0])
}

fn emoji(ws: &WorkspaceId, i: usize) -> String {
    slakio_world::emoji::picture(ws.as_str(), &format!("e{i:02}"))
}

/// Booted at 200x110, pictures and photos on, `deploys` open with 20 messages by 20 people with
/// photos, each with two of the workspace's 40 emoji; the list asked for and given. The pictures
/// the first frame asked for with the list.
fn busy() -> (Scripted, WorkspaceId, Vec<Request>) {
    let mut s = Scripted::booted(true, None);
    s.d.app.images.env = slakio_tui::images::detect::Env { term: "xterm-kitty".into(), ..Default::default() };
    (s.d.app.images.setting, s.d.app.images.tty) = ("auto".into(), true);
    s.d.app.images.resolve();
    s.d.app.settings.photos = true;
    s.d.app.resize(200, 110);
    s.d.list_cursor_on("deploys");
    s.keys("enter");
    let at = s.b.held.iter().position(|r| matches!(r.command, Command::History { .. })).expect("the page asked");
    let mut e = s.b.reply(at);
    let Event::History(page) = &mut e.event else { panic!("a page") };
    let ws = page.target.workspace().clone();
    page.users = (0..PEOPLE)
        .map(|i| User {
            id: UserId::new(format!("UP{i:02}")),
            workspace: ws.clone(),
            name: Remote::new(format!("person{i:02}")),
            display_name: Remote::new(format!("Person {i:02}")),
            org: Org::Own,
            bot: false,
            presence: Presence::Unknown,
            avatar: Some(photo(&ws, i)),
            no_photo: false,
            avatar_guessed: false,
        })
        .collect();
    page.messages = (0..PEOPLE)
        .map(|k| Message {
            ts: Ts(1_767_603_600_000_000 + k as u64 * 60_000_000),
            user: UserId::new(format!("UP{k:02}")),
            text: Remote::new(format!("note :e{:02}: and :e{:02}:", 2 * k, 2 * k + 1)),
            thread: None,
            reactions: Vec::new(),
            edited: false,
            files: Vec::new(),
        })
        .collect();
    page.complete = true;
    s.b.push(e);
    s.pump();
    let asked = frame(&mut s);
    let list = asked.iter().find(|r| r.command == Command::Emoji).expect("the list asked");
    let set = (0..EMOJI).map(|i| (format!("e{i:02}"), Custom::Picture(emoji(&ws, i)))).collect();
    s.b.push(Envelope { generation: list.id, event: Event::Emoji { emoji: set } });
    s.pump();
    (s, ws, asked.into_iter().filter(|r| r.command != Command::Emoji).collect())
}

/// A frame, as the binary's loop draws one: what it asked for.
fn frame(s: &mut Scripted) -> Vec<Request> {
    s.d.screen();
    s.d.app.ask_images(s.d.now);
    s.pump();
    let (asked, kept) = s.b.held.drain(..).partition(|r| matches!(r.command, Command::Image { .. } | Command::Emoji));
    s.b.held = kept;
    asked
}

/// Picture request `r` answered and decoded as the binary does.
fn give(s: &mut Scripted, r: &Request) {
    s.b.push(Envelope { generation: r.id, event: Event::Image { bytes: slakio_world::pictures::png("p", 72, 72) } });
    s.pump();
    for effect in s.d.app.take_effects() {
        if let Effect::Decode(job) = effect {
            let result = prepare(&job, Some((9, 18)));
            s.d.app.images.decoded((job.source, job.request), result);
        }
    }
}

fn source(r: &Request) -> Source {
    match &r.command {
        Command::Image { source, .. } => source.clone(),
        other => panic!("{other:?}"),
    }
}

fn ready(look: Look) -> bool {
    matches!(look, Look::Ready { .. })
}

#[test]
fn twenty_photos_and_the_frame_s_emoji_fill_the_screen_in_order_a_frame_per_twelve() {
    let (mut s, ws, first) = busy();
    let (mut order, mut frames) = (Vec::new(), 0);
    let mut next = Some(first);
    loop {
        let asked = next.take().unwrap_or_else(|| frame(&mut s));
        if asked.is_empty() {
            break;
        }
        frames += 1;
        assert!(asked.len() <= CDN_IN_FLIGHT_MAX, "{}", asked.len());
        for r in &asked {
            order.push(source(r));
            give(&mut s, r);
        }
        assert!(frames < 40, "still asking after 40 frames");
    }
    let screen = s.d.screen();
    let photos: Vec<usize> =
        (0..PEOPLE).filter(|i| ready(s.d.app.images.look(&ws, &Source::Avatar(photo(&ws, *i))))).collect();
    assert_eq!(photos.len(), PEOPLE, "every photo drawn: {photos:?}");
    let drawn: Vec<usize> = (0..EMOJI)
        .filter(|i| ready(s.d.app.images.look_as(&ws, &Source::Emoji(emoji(&ws, *i)), Shape::Emoji)))
        .collect();
    assert_eq!(drawn.len(), EMOJI_PER_FRAME, "the frame's share of emoji drawn: {drawn:?}");
    assert!(screen.contains(":e00:") && !screen.contains(":e39:"), "past the share, text:\n{screen}");
    // Each frame asks twelve, the next frame the next twelve: no frame without an ask until done.
    let timeline = PEOPLE + EMOJI_PER_FRAME;
    assert_eq!(order.len(), timeline, "each asked once, nothing of the list's past the timeline's");
    assert_eq!(frames, timeline.div_ceil(CDN_IN_FLIGHT_MAX), "{frames} frames for {timeline} pictures");
    // The frame before the list asked the twelve newest photos. Then the newest message first
    // (where the timeline starts drawing), its emoji left to right before its author's photo; the
    // oldest four messages' emoji are past the frame's share.
    let early = PEOPLE - CDN_IN_FLIGHT_MAX;
    let mut want: Vec<Source> = (early..PEOPLE).rev().map(|k| Source::Avatar(photo(&ws, k))).collect();
    for k in (0..PEOPLE).rev() {
        if k >= PEOPLE - EMOJI_PER_FRAME / 2 {
            want.push(Source::Emoji(emoji(&ws, 2 * k)));
            want.push(Source::Emoji(emoji(&ws, 2 * k + 1)));
        }
        if k < early {
            want.push(Source::Avatar(photo(&ws, k)));
        }
    }
    assert_eq!(order, want);
}

#[test]
fn a_picture_that_failed_for_the_budget_wakes_the_loop_when_it_may_be_asked_again() {
    let (mut s, _, asked) = busy();
    let first = asked.first().expect("a picture").clone();
    s.b.held.push(first.clone());
    s.b.fail(s.b.held.len() - 1, FailKind::CapReached);
    s.pump();
    let at = s.d.app.deadline().expect("a wake for the retry");
    assert!(at > s.d.now, "later, not at once");
    assert!(!s.d.app.on_tick(s.d.now), "not before");
    for r in &asked[1..] {
        give(&mut s, r);
    }
    assert!(s.d.app.on_tick(at), "the wait is over: the screen is drawn again");
    s.d.now = at;
    let again = frame(&mut s);
    assert!(again.iter().any(|r| source(r) == source(&first)), "asked again with no key pressed");
}
