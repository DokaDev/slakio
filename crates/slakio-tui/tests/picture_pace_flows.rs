//! The pace of a busy channel's pictures, on a clock the test moves: every photo and emoji a
//! screen shows is drawn within a few seconds when each takes the backend 300 ms, the photos and
//! emoji asked a dozen at once (the file previews keep to four).

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use scripted::Scripted;
use slakio_core::backend::{Command, Envelope, Event, Generation};
use slakio_core::images::emoji::Custom;
use slakio_core::images::{AVATAR_HOSTS, Source};
use slakio_core::model::{Message, Ts, User, UserId, WorkspaceId};
use slakio_core::sanitize::Remote;
use slakio_tui::app::Effect;
use slakio_tui::images::decode::Shape;
use slakio_tui::images::{Look, PHOTO, prepare};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long the backend takes for each picture.
const ANSWER: Duration = Duration::from_millis(300);
/// The loop's step.
const STEP: Duration = Duration::from_millis(100);

/// Booted, pictures and photos on as the binary sets them in kitty, `w`×`h`.
fn kitty(w: u16, h: u16) -> Scripted {
    let mut s = Scripted::booted(true, None);
    s.d.app.images.env = slakio_tui::images::detect::Env { term: "xterm-kitty".into(), ..Default::default() };
    (s.d.app.images.setting, s.d.app.images.tty) = ("auto".into(), true);
    s.d.app.images.resolve();
    assert!(s.d.app.images.on);
    s.d.app.settings.photos = true;
    s.d.app.resize(w, h);
    s
}

/// Person `tag` of workspace `ws`, with a photo of their own on the avatar host.
fn person(s: &Scripted, ws: &WorkspaceId, like: &UserId, tag: &str) -> User {
    let mut user = s.d.app.model.user(ws, like).cloned().expect("a person to copy");
    user.id = UserId::new(format!("U{tag}"));
    user.name = Remote::new(format!("person.{tag}"));
    user.display_name = Remote::new(format!("Person {tag}"));
    user.avatar = Some(format!("https://{}/2026-01-05/{ws}-{tag}_72.png", AVATAR_HOSTS[0]));
    user.no_photo = false;
    user
}

/// `name` opened with one message by each of `texts`' people (their tags, their text): the
/// pictures the screen should show (each person's photo, each emoji named).
fn open(
    s: &mut Scripted,
    name: &str,
    texts: &[(String, String)],
    emoji: &[String],
) -> (WorkspaceId, Vec<(Source, Shape)>) {
    s.d.list_cursor_on(name);
    s.keys("enter");
    let i = s.b.held.iter().position(|r| matches!(r.command, Command::History { .. })).expect("the page asked");
    let mut e = s.b.reply(i);
    let Event::History(page) = &mut e.event else { panic!("a page") };
    let ws = page.target.workspace().clone();
    let like = page.messages[0].user.clone();
    let people: Vec<User> = texts.iter().map(|(tag, _)| person(s, &ws, &like, tag)).collect();
    s.d.app.model.add_users(&ws, &people);
    page.messages = (texts.iter().zip(&people).enumerate())
        .map(|(k, ((_, text), user))| Message {
            ts: Ts(1_767_603_600_000_000 + k as u64 * 60_000_000),
            user: user.id.clone(),
            text: Remote::new(text),
            thread: None,
            reactions: Vec::new(),
            edited: false,
            emoji_pictures: Vec::new(),
            files: Vec::new(),
        })
        .collect();
    page.complete = true;
    s.b.push(e);
    s.pump();
    let mut want: Vec<(Source, Shape)> =
        people.iter().map(|u| (Source::Avatar(u.avatar.clone().unwrap()), PHOTO)).collect();
    want.extend(emoji.iter().map(|n| (Source::Emoji(slakio_world::emoji::picture(ws.as_str(), n)), Shape::Emoji)));
    (ws, want)
}

/// `n` people tagged `prefix0..`, each saying one of `emoji` (and the rest, one more each, from
/// the first message on).
fn messages(prefix: &str, n: usize, emoji: &[String]) -> Vec<(String, String)> {
    (0..n)
        .map(|k| {
            let mut text = format!("note {k} :{}:", emoji[k]);
            if let Some(more) = emoji.get(n + k) {
                text.push_str(&format!(" and :{more}:"));
            }
            (format!("{prefix}{k}"), text)
        })
        .collect()
}

/// Every one of `want` is drawn.
fn all_ready(s: &Scripted, ws: &WorkspaceId, want: &[(Source, Shape)]) -> bool {
    want.iter().all(|(source, shape)| matches!(s.d.app.images.look_as(ws, source, *shape), Look::Ready { .. }))
}

/// The binary's loop over a backend that answers each picture [`ANSWER`] after it was asked
/// (and the emoji list at once with `list`): a frame each [`STEP`], until every one of `want` is
/// drawn. How long that took.
fn until_drawn(s: &mut Scripted, ws: &WorkspaceId, want: &[(Source, Shape)], list: &[(String, Custom)]) -> Duration {
    let start = s.d.now;
    let mut asked: HashMap<Generation, Instant> = HashMap::new();
    let mut sources: HashMap<Generation, Source> = HashMap::new();
    loop {
        s.d.screen();
        s.d.app.ask_images(s.d.now);
        s.pump();
        if let Some(i) = s.b.held.iter().position(|r| r.command == Command::Emoji) {
            let r = s.b.held.remove(i);
            s.b.push(Envelope { generation: r.id, event: Event::Emoji { emoji: list.to_vec(), truncated: false } });
            s.pump();
            continue;
        }
        for r in s.b.held.iter().filter(|r| matches!(r.command, Command::Image { .. })) {
            asked.entry(r.id).or_insert(s.d.now);
            if let Command::Image { source, .. } = &r.command {
                sources.entry(r.id).or_insert_with(|| source.clone());
            }
        }
        s.d.screen();
        if all_ready(s, ws, want) {
            let mut each: Vec<&Source> = sources.values().collect();
            each.sort_by_key(|s| s.address().to_string());
            let before = each.len();
            each.dedup();
            assert_eq!(each.len(), before, "each picture asked once");
            return s.d.now - start;
        }
        assert!(s.d.now - start < Duration::from_secs(120), "never all drawn");
        s.d.now += STEP;
        let due: Vec<Generation> = (s.b.held.iter())
            .filter(|r| asked.get(&r.id).is_some_and(|at| s.d.now - *at >= ANSWER))
            .map(|r| r.id)
            .collect();
        for id in due {
            let i = s.b.held.iter().position(|r| r.id == id).unwrap();
            s.b.held.remove(i);
            let bytes = slakio_world::pictures::png(&format!("{id:?}"), 64, 64);
            s.b.push(Envelope { generation: id, event: Event::Image { bytes } });
            s.pump();
        }
        for effect in s.d.app.take_effects() {
            if let Effect::Decode(job) = effect {
                let result = prepare(&job, Some((9, 18)));
                s.d.app.images.decoded((job.source, job.request), result);
            }
        }
    }
}

#[test]
fn a_busy_channel_draws_every_screen_within_three_seconds() {
    let mut s = kitty(200, 120);
    let names: Vec<String> = (0..60).map(|k| format!("pace-{k}")).collect();
    // Screen one: 24 people's photos and 30 emoji; screen two: 16 more and 30 more.
    let (first, second) = (&names[..30], &names[30..]);
    let (ws, want) = open(&mut s, "deploys", &messages("A", 24, first), first);
    assert_eq!(want.len(), 54);
    let list: Vec<(String, Custom)> =
        names.iter().map(|n| (n.clone(), Custom::Picture(slakio_world::emoji::picture(ws.as_str(), n)))).collect();
    let took = until_drawn(&mut s, &ws, &want, &list);
    assert!(took <= Duration::from_secs(3), "screen one took {took:?}");
    let (ws, want) = open(&mut s, "backend", &messages("B", 16, second), second);
    assert_eq!(want.len(), 46);
    let took = until_drawn(&mut s, &ws, &want, &list);
    assert!(took <= Duration::from_secs(3), "screen two took {took:?}");
}
