use super::*;
use slakio_core::model::FILE_HOST;

fn ws() -> WorkspaceId {
    WorkspaceId::new("T1")
}

fn thumb(n: usize) -> Source {
    Source::Thumb(format!("https://{FILE_HOST}/files-tmb/T1-F{n}/x.png"))
}

fn on() -> Images {
    Images { on: true, ..Images::default() }
}

fn decoded(png: usize) -> Decoded {
    Decoded { png: vec![0; png], cols: 3, rows: 2 }
}

/// `source` drawn, asked for and answered: its job.
fn fetch(images: &mut Images, requests: &mut Requests, source: &Source) -> Job {
    let now = Instant::now();
    assert_eq!(images.look(&ws(), source), Look::Pending);
    assert!(images.ask(requests, now));
    let r = requests.take().pop().expect("asked");
    assert_eq!(r.command, Command::Image { source: source.clone(), max: BYTES_MAX });
    assert_eq!(r.workspace, Some(ws()));
    images.answered(r.id, vec![1, 2, 3]).expect("a job")
}

#[test]
fn off_nothing_is_wanted_or_asked() {
    let (mut images, mut requests) = (Images::default(), Requests::default());
    assert_eq!(images.look(&ws(), &thumb(1)), Look::Pending);
    assert!(!images.ask(&mut requests, Instant::now()));
    assert!(requests.take().is_empty());
}

#[test]
fn a_picture_drawn_is_asked_for_once_decoded_and_sent_to_the_terminal() {
    let (mut images, mut requests) = (on(), Requests::default());
    let job = fetch(&mut images, &mut requests, &thumb(1));
    assert_eq!(job.shape, Shape::Preview);
    // Drawn again while it comes: not asked again.
    assert_eq!(images.look(&ws(), &thumb(1)), Look::Pending);
    assert!(!images.ask(&mut requests, Instant::now()));
    images.decoded(thumb(1), Ok(decoded(10)));
    let sent = images.take_graphics();
    assert!(sent.starts_with("\x1b_Ga=T,U=1,f=100,t=d,i=1,c=3,r=2,q=2"), "{sent:?}");
    assert_eq!(images.take_graphics(), "", "sent once");
    assert_eq!(images.look(&ws(), &thumb(1)), Look::Ready { id: 1, cols: 3, rows: 2 });
}

#[test]
fn at_most_a_few_are_asked_at_once() {
    let (mut images, mut requests) = (on(), Requests::default());
    for n in 0..10 {
        images.look(&ws(), &thumb(n));
    }
    images.ask(&mut requests, Instant::now());
    assert_eq!(requests.take().len(), IN_FLIGHT_MAX);
}

#[test]
fn a_failure_says_why_and_only_a_passing_one_is_asked_again_later() {
    let (mut images, mut requests) = (on(), Requests::default());
    let now = Instant::now();
    for (n, kind, why, again) in [
        (1, FailKind::TooLarge, Why::TooLarge, false),
        (2, FailKind::Api { code: "not_an_image".into() }, Why::Unreadable, false),
        (3, FailKind::Api { code: "file_not_available".into() }, Why::NotLoaded, false),
        (4, FailKind::Network, Why::NotLoaded, true),
        (5, FailKind::CapReached, Why::NotLoaded, true),
    ] {
        images.look(&ws(), &thumb(n));
        images.ask(&mut requests, now);
        let r = requests.take().pop().unwrap();
        assert!(images.failed(r.id, &kind, now));
        assert_eq!(images.look(&ws(), &thumb(n)), Look::Failed(why), "{kind:?}");
        assert!(!images.ask(&mut requests, now), "{kind:?}: not at once");
        images.look(&ws(), &thumb(n));
        assert_eq!(images.ask(&mut requests, now + RETRY_AFTER), again, "{kind:?}");
        requests.take();
    }
    assert!(!images.failed(Generation(999), &FailKind::Network, now), "not a picture's request");
}

#[test]
fn a_refused_picture_is_never_asked_again() {
    let (mut images, mut requests) = (on(), Requests::default());
    let job = fetch(&mut images, &mut requests, &thumb(1));
    images.decoded(job.source, Err(Refused::TooLarge));
    assert_eq!(images.look(&ws(), &thumb(1)), Look::Failed(Why::TooLarge));
    assert!(!images.ask(&mut requests, Instant::now() + RETRY_AFTER * 2));
    assert_eq!(images.take_graphics(), "");
}

#[test]
fn the_terminal_holds_at_most_so_many_pictures_and_bytes() {
    let (mut images, mut requests) = (on(), Requests::default());
    for n in 0..ENTRIES_MAX + 3 {
        let job = fetch(&mut images, &mut requests, &thumb(n));
        images.decoded(job.source, Ok(decoded(100)));
    }
    assert_eq!(images.held().0, ENTRIES_MAX);
    let out = images.take_graphics();
    assert!(out.contains("a=d,d=I,i=1,q=2") && out.contains("a=d,d=I,i=3,q=2"), "the oldest went");
    assert!(!out.contains("a=d,d=I,i=4,"));
    assert_eq!(images.look(&ws(), &thumb(0)), Look::Pending, "gone: fetched again when drawn");
    // Bytes: a large one pushes the others out, never itself.
    let job = fetch(&mut images, &mut requests, &thumb(1000));
    images.decoded(job.source, Ok(decoded(BYTES_CAP)));
    assert_eq!(images.held(), (1, BYTES_CAP));
}

#[test]
fn the_least_recently_drawn_goes_first() {
    let (mut images, mut requests) = (on(), Requests::default());
    for n in 0..ENTRIES_MAX {
        let job = fetch(&mut images, &mut requests, &thumb(n));
        images.decoded(job.source, Ok(decoded(10)));
    }
    images.take_graphics();
    // The first one is drawn again: the second is now the oldest.
    images.look(&ws(), &thumb(0));
    images.ask(&mut requests, Instant::now());
    let job = fetch(&mut images, &mut requests, &thumb(999));
    images.decoded(job.source, Ok(decoded(10)));
    let out = images.take_graphics();
    assert!(out.contains("a=d,d=I,i=2,q=2"), "{out:?}");
    assert!(matches!(images.look(&ws(), &thumb(0)), Look::Ready { .. }));
}

#[test]
fn a_late_or_unknown_answer_changes_nothing() {
    let (mut images, mut requests) = (on(), Requests::default());
    assert!(images.answered(Generation(5), vec![1]).is_none());
    images.decoded(thumb(1), Ok(decoded(10)));
    assert_eq!(images.take_graphics(), "", "never asked: not sent");
    let job = fetch(&mut images, &mut requests, &thumb(2));
    images.decoded(job.source.clone(), Ok(decoded(10)));
    images.decoded(job.source, Ok(decoded(10)));
    assert_eq!(images.held().0, 1, "decoded twice: held once");
}

#[test]
fn a_photo_is_decoded_for_the_picture_slot() {
    let (mut images, mut requests) = (on(), Requests::default());
    let photo = Source::Avatar(format!("https://{}/x.png", slakio_core::images::AVATAR_HOSTS[0]));
    assert_eq!(fetch(&mut images, &mut requests, &photo).shape, Shape::Photo { cols: 4, rows: 2 });
}

fn kitty() -> detect::Env {
    detect::Env { term: "xterm-kitty".into(), ..detect::Env::default() }
}

#[test]
fn turned_off_every_picture_leaves_the_terminal_at_once() {
    let mut images = Images { env: kitty(), setting: "auto".into(), tty: true, ..Images::default() };
    images.resolve();
    assert!(images.on);
    let mut requests = Requests::default();
    let job = fetch(&mut images, &mut requests, &thumb(1));
    images.decoded(job.source, Ok(decoded(10)));
    images.take_graphics();
    assert_eq!(images.choose("off"), Chosen::Done);
    assert!(!images.on);
    assert_eq!(images.take_graphics(), "\x1b_Ga=d,d=A,q=2\x1b\\", "all of them, at once");
    assert_eq!(images.look(&ws(), &thumb(1)), Look::Pending);
    assert!(!images.ask(&mut requests, Instant::now()), "nothing asked while off");
    assert_eq!(images.held(), (0, 0));
    // On again: what is drawn is asked for again.
    assert_eq!(images.choose("auto"), Chosen::Done);
    assert_eq!(images.take_graphics(), "", "nothing written turning on");
    images.look(&ws(), &thumb(1));
    assert!(images.ask(&mut requests, Instant::now()));
}

#[test]
fn on_where_slakio_cannot_tell_is_said_and_the_variable_pins_the_choice() {
    let plain = detect::Env { term: "xterm-256color".into(), ..detect::Env::default() };
    let mut images = Images { env: plain, setting: "auto".into(), tty: true, ..Images::default() };
    images.resolve();
    assert!(!images.on);
    assert_eq!(images.choose("on"), Chosen::Forced);
    assert!(images.on);
    let pinned = detect::Env { images: Some("off".into()), ..kitty() };
    let mut images = Images { env: pinned, setting: "auto".into(), tty: true, ..Images::default() };
    images.resolve();
    assert_eq!(images.choose("on"), Chosen::Pinned("off".into()));
    assert!(!images.on, "the variable wins");
    assert_eq!(images.choose("off"), Chosen::Done, "the same as the variable: nothing to say");
}

#[test]
fn leaving_photos_removes_them_from_the_terminal_and_keeps_previews() {
    let (mut images, mut requests) = (on(), Requests::default());
    let photo = Source::Avatar(format!("https://{}/x.png", slakio_core::images::AVATAR_HOSTS[0]));
    for s in [thumb(1), photo.clone()] {
        let job = fetch(&mut images, &mut requests, &s);
        images.decoded(job.source, Ok(decoded(10)));
    }
    images.take_graphics();
    images.forget_photos();
    assert_eq!(images.take_graphics(), "\x1b_Ga=d,d=I,i=2,q=2\x1b\\");
    assert_eq!(images.held(), (1, 10));
    assert!(matches!(images.look(&ws(), &thumb(1)), Look::Ready { .. }));
}
