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

/// `job` made ready as a picture of `png` bytes, 3 × 2 cells.
fn ready(job: &Job, png: usize) -> Prepared {
    let bytes = vec![0; png];
    Prepared {
        id: job.id,
        cols: 3,
        rows: 2,
        bytes: png,
        transmit: graphics::transmit(job.id, 3, 2, &bytes),
        from: (6, 4, "png"),
    }
}

fn key(job: &Job) -> (Source, Generation) {
    (job.source.clone(), job.request)
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
    images.decoded(key(&job), Ok(ready(&job, 10)));
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
    images.decoded(key(&job), Err(Refused::TooLarge));
    assert_eq!(images.look(&ws(), &thumb(1)), Look::Failed(Why::TooLarge));
    assert!(!images.ask(&mut requests, Instant::now() + RETRY_AFTER * 2));
    assert_eq!(images.take_graphics(), "");
}

#[test]
fn the_terminal_holds_at_most_so_many_pictures_and_bytes() {
    let (mut images, mut requests) = (on(), Requests::default());
    for n in 0..ENTRIES_MAX + 3 {
        let job = fetch(&mut images, &mut requests, &thumb(n));
        images.decoded(key(&job), Ok(ready(&job, 100)));
    }
    assert_eq!(images.held().0, ENTRIES_MAX);
    let out = images.take_graphics();
    assert!(out.contains("a=d,d=I,i=1,q=2") && out.contains("a=d,d=I,i=3,q=2"), "the oldest went");
    assert!(!out.contains("a=d,d=I,i=4,"));
    assert_eq!(images.look(&ws(), &thumb(0)), Look::Pending, "gone: fetched again when drawn");
    // A frame drawing none of those held: they are no longer on screen.
    images.ask(&mut requests, Instant::now());
    requests.take();
    // Bytes: a large one pushes the others out, never itself.
    let job = fetch(&mut images, &mut requests, &thumb(1000));
    images.decoded(key(&job), Ok(ready(&job, BYTES_CAP)));
    assert_eq!(images.held(), (1, BYTES_CAP));
}

#[test]
fn the_least_recently_drawn_goes_first() {
    let (mut images, mut requests) = (on(), Requests::default());
    for n in 0..ENTRIES_MAX {
        let job = fetch(&mut images, &mut requests, &thumb(n));
        images.decoded(key(&job), Ok(ready(&job, 10)));
    }
    images.take_graphics();
    // The first one is drawn again: the second is now the oldest.
    images.look(&ws(), &thumb(0));
    images.ask(&mut requests, Instant::now());
    let job = fetch(&mut images, &mut requests, &thumb(999));
    images.decoded(key(&job), Ok(ready(&job, 10)));
    let out = images.take_graphics();
    assert!(out.contains("a=d,d=I,i=2,q=2"), "{out:?}");
    assert!(matches!(images.look(&ws(), &thumb(0)), Look::Ready { .. }));
}

#[test]
fn a_late_or_unknown_answer_changes_nothing() {
    let (mut images, mut requests) = (on(), Requests::default());
    assert!(images.answered(Generation(5), vec![1]).is_none());
    let stray = Job {
        source: thumb(1),
        request: Generation(77),
        id: 9,
        bytes: Vec::new(),
        shape: Shape::Preview,
        from_cache: false,
    };
    images.decoded(key(&stray), Ok(ready(&stray, 10)));
    assert_eq!(images.take_graphics(), "", "never asked: not sent");
    let job = fetch(&mut images, &mut requests, &thumb(2));
    images.decoded(key(&job), Ok(ready(&job, 10)));
    images.decoded(key(&job), Ok(ready(&job, 10)));
    assert_eq!(images.held().0, 1, "decoded twice: held once");
}

#[test]
fn a_photo_is_decoded_for_the_picture_slot() {
    let (mut images, mut requests) = (on(), Requests::default());
    let photo = Source::Avatar(format!("https://{}/x.png", slakio_core::images::AVATAR_HOSTS[0]));
    assert_eq!(fetch(&mut images, &mut requests, &photo).shape, Shape::Photo { cols: 4, rows: 2 });
}

#[test]
fn a_photo_in_the_list_s_chip_is_a_picture_of_its_own_beside_the_slot_s() {
    let (mut images, mut requests) = (on(), Requests::default());
    let photo = Source::Avatar(format!("https://{}/x.png", slakio_core::images::AVATAR_HOSTS[0]));
    let slot = fetch(&mut images, &mut requests, &photo);
    assert_eq!(images.look_as(&ws(), &photo, CHIP_PHOTO), Look::Pending, "the chip's is not the slot's");
    assert!(images.ask(&mut requests, Instant::now()));
    let r = requests.take().pop().expect("asked for the chip");
    assert_eq!(r.command, Command::Image { source: photo.clone(), max: BYTES_MAX });
    let chip = images.answered(r.id, vec![1]).expect("a job");
    assert_eq!(chip.shape, Shape::Photo { cols: 2, rows: 1 });
    assert_ne!(chip.id, slot.id);
    // Each result goes to its own picture, whichever comes first.
    let made = |job: &Job, cols, rows| Prepared {
        id: job.id,
        cols,
        rows,
        bytes: 5,
        transmit: graphics::transmit(job.id, cols, rows, &[0; 5]),
        from: (4, 4, "png"),
    };
    images.decoded(key(&chip), Ok(made(&chip, 2, 1)));
    images.decoded(key(&slot), Ok(made(&slot, 4, 2)));
    assert_eq!(images.look_as(&ws(), &photo, CHIP_PHOTO), Look::Ready { id: chip.id, cols: 2, rows: 1 });
    assert_eq!(images.look(&ws(), &photo), Look::Ready { id: slot.id, cols: 4, rows: 2 });
    assert_eq!(images.held(), (2, 10));
    // Photos off: both leave the terminal.
    images.take_graphics();
    images.forget_photos();
    let sent = images.take_graphics();
    assert!(sent.contains(&graphics::delete(chip.id)) && sent.contains(&graphics::delete(slot.id)), "{sent:?}");
    assert_eq!(images.held(), (0, 0));
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
    images.decoded(key(&job), Ok(ready(&job, 10)));
    images.take_graphics();
    assert_eq!(images.choose("off"), Chosen::Done);
    assert!(!images.on);
    assert_eq!(
        images.take_graphics(),
        "\x1b_Ga=d,d=I,i=1,q=2\x1b\\\x1b_Ga=d,d=A,q=2\x1b\\",
        "each by its id (a virtual placement may outlive a delete of all), then all"
    );
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
        images.decoded(key(&job), Ok(ready(&job, 10)));
    }
    images.take_graphics();
    images.forget_photos();
    assert_eq!(images.take_graphics(), "\x1b_Ga=d,d=I,i=2,q=2\x1b\\");
    assert_eq!(images.held(), (1, 10));
    assert!(matches!(images.look(&ws(), &thumb(1)), Look::Ready { .. }));
}

#[test]
fn a_result_for_a_request_no_longer_waited_for_changes_nothing() {
    let mut images = Images { env: kitty(), setting: "auto".into(), tty: true, ..Images::default() };
    images.resolve();
    let mut requests = Requests::default();
    let old = fetch(&mut images, &mut requests, &thumb(1));
    images.choose("off");
    images.choose("auto");
    images.take_graphics();
    let new = fetch(&mut images, &mut requests, &thumb(1));
    assert_ne!(old.request, new.request);
    images.decoded(key(&old), Ok(ready(&old, 10)));
    assert_eq!(images.take_graphics(), "", "the old request's picture is not sent");
    assert_eq!(images.look(&ws(), &thumb(1)), Look::Pending);
    images.decoded(key(&new), Ok(ready(&new, 10)));
    assert!(matches!(images.look(&ws(), &thumb(1)), Look::Ready { id, .. } if id == new.id));
}

#[test]
fn what_a_frame_only_laid_out_is_neither_asked_for_nor_drawn() {
    let (mut images, mut requests) = (on(), Requests::default());
    let held = fetch(&mut images, &mut requests, &thumb(0));
    images.decoded(key(&held), Ok(ready(&held, 10)));
    images.look(&ws(), &thumb(1));
    let from = images.mark();
    // A message measured above the screen: its pictures are only laid out.
    images.look(&ws(), &thumb(2));
    images.look(&ws(), &thumb(0));
    images.not_shown(from, images.mark());
    images.look(&ws(), &thumb(3));
    images.ask(&mut requests, Instant::now());
    let asked: Vec<Source> = requests
        .take()
        .into_iter()
        .filter_map(|r| match r.command {
            Command::Image { source, .. } => Some(source),
            _ => None,
        })
        .collect();
    assert_eq!(asked, [thumb(1), thumb(3)]);
}

#[test]
fn a_busy_or_offline_backend_is_asked_again_soon() {
    let (mut images, mut requests) = (on(), Requests::default());
    let now = Instant::now();
    for (n, kind) in [(1, FailKind::Busy), (2, FailKind::Offline)] {
        images.look(&ws(), &thumb(n));
        images.ask(&mut requests, now);
        let r = requests.take().pop().unwrap();
        images.failed(r.id, &kind, now);
        images.look(&ws(), &thumb(n));
        assert!(!images.ask(&mut requests, now + RETRY_SOON / 2), "{kind:?}");
        images.look(&ws(), &thumb(n));
        assert!(images.ask(&mut requests, now + RETRY_SOON), "{kind:?}: in seconds, not a minute");
        requests.take();
    }
}

#[test]
fn failures_kept_are_bounded() {
    let (mut images, mut requests) = (on(), Requests::default());
    let now = Instant::now();
    for n in 0..2000 {
        images.look(&ws(), &thumb(n));
        images.ask(&mut requests, now);
        let r = requests.take().pop().unwrap();
        images.failed(r.id, &FailKind::TooLarge, now);
    }
    assert_eq!(images.failures(), FAILED_MAX);
    assert!(images.entries.len() <= FAILED_MAX);
    assert_eq!(images.look(&ws(), &thumb(1999)), Look::Failed(Why::TooLarge), "the newest are kept");
}

#[test]
fn photos_on_their_way_are_not_waited_for_once_photos_are_off() {
    let (mut images, mut requests) = (on(), Requests::default());
    let photo = Source::Avatar(format!("https://{}/x.png", slakio_core::images::AVATAR_HOSTS[0]));
    images.look(&ws(), &photo);
    images.ask(&mut requests, Instant::now());
    let r = requests.take().pop().unwrap();
    images.forget_photos();
    assert!(images.answered(r.id, vec![1]).is_none(), "its answer is dropped");
    assert_eq!(images.held(), (0, 0));
}

#[test]
fn auto_where_the_terminal_shows_no_pictures_says_so() {
    let plain = detect::Env { term: "xterm-256color".into(), ..detect::Env::default() };
    let mut images = Images { env: plain, setting: "off".into(), tty: true, ..Images::default() };
    images.resolve();
    assert_eq!(images.choose("auto"), Chosen::NotHere);
    assert!(!images.on);
}

/// One shown frame drawing `shown`, then what it asked for, each answered and made ready at once
/// unless `hold`: the requests (still on their way when held).
fn frame_of(images: &mut Images, requests: &mut Requests, shown: &[Source], hold: bool) -> Vec<Generation> {
    for source in shown {
        images.look(&ws(), source);
    }
    images.ask(requests, Instant::now());
    let mut out = Vec::new();
    for r in requests.take() {
        out.push(r.id);
        if !hold {
            let job = images.answered(r.id, vec![1]).expect("a job");
            images.decoded(key(&job), Ok(ready(&job, 10)));
        }
    }
    out
}

#[test]
fn more_pictures_on_screen_than_the_terminal_holds_are_asked_once_and_none_shown_is_removed() {
    let (mut images, mut requests) = (on(), Requests::default());
    let shown: Vec<Source> = (0..ENTRIES_MAX + 10).map(thumb).collect();
    let mut asked = 0;
    for round in 0..40 {
        let now = frame_of(&mut images, &mut requests, &shown, false);
        assert!(now.len() <= IN_FLIGHT_MAX);
        asked += now.len();
        assert!(!images.take_graphics().contains("a=d"), "round {round}: a picture on screen left the terminal");
    }
    assert_eq!(asked, ENTRIES_MAX, "each asked once, as many as the terminal holds");
    assert_eq!(images.held().0, ENTRIES_MAX);
    assert!(frame_of(&mut images, &mut requests, &shown, false).is_empty(), "settled");
    let ready = shown.iter().filter(|s| matches!(images.look(&ws(), s), Look::Ready { .. })).count();
    assert_eq!(ready, ENTRIES_MAX, "the rest stand in for themselves");
}

#[test]
fn a_picture_with_no_room_left_is_parked_until_the_screen_changes() {
    let (mut images, mut requests) = (on(), Requests::default());
    let held: Vec<Source> = (0..ENTRIES_MAX).map(thumb).collect();
    for _ in 0..ENTRIES_MAX {
        frame_of(&mut images, &mut requests, &held, false);
    }
    assert_eq!(images.held().0, ENTRIES_MAX);
    images.take_graphics();
    // Only the newcomer on screen for two frames: the others may make room, so it is asked.
    let extra = [thumb(999)];
    assert!(frame_of(&mut images, &mut requests, &extra, true).is_empty(), "the frame before still showed them all");
    let ids = frame_of(&mut images, &mut requests, &extra, true);
    assert_eq!(ids.len(), 1);
    // They are all back on screen with it before its bytes come: nothing shown is removed, it
    // waits.
    let all: Vec<Source> = held.iter().chain(&extra).cloned().collect();
    frame_of(&mut images, &mut requests, &all, true);
    let job = images.answered(ids[0], vec![1]).expect("a job");
    images.decoded(key(&job), Ok(ready(&job, 10)));
    assert_eq!(images.take_graphics(), "", "neither sent nor anything removed");
    assert_eq!(images.held().0, ENTRIES_MAX);
    // The same screen: parked, not asked.
    for _ in 0..3 {
        assert!(frame_of(&mut images, &mut requests, &all, true).is_empty());
        assert_eq!(images.entries.get(&(extra[0].clone(), Shape::Preview)), Some(&Entry::Parked));
    }
    // The screen changes: asked again once there is room (the frame before still showed the
    // others).
    assert!(frame_of(&mut images, &mut requests, &extra, true).is_empty());
    assert_eq!(frame_of(&mut images, &mut requests, &extra, true).len(), 1);
}

#[test]
fn a_frame_draws_at_most_so_many_emoji_pictures() {
    let images = on();
    let address = |n: usize| format!("https://{}/T1/e{n}/0.png", slakio_core::images::EMOJI_HOSTS[0]);
    for n in 0..EMOJI_PER_FRAME + 5 {
        images.look_emoji(&ws(), &address(n));
    }
    // One already looked at is looked at again, whatever the count.
    images.look_emoji(&ws(), &address(0));
    let wanted: Vec<Source> = images.wanted.borrow().iter().map(|w| w.1.0.clone()).collect();
    assert_eq!(wanted.len(), EMOJI_PER_FRAME + 1);
    assert!(wanted.iter().all(|s| *s != Source::Emoji(address(EMOJI_PER_FRAME))));
}

#[test]
fn a_list_photo_yields_its_room_to_a_timeline_picture() {
    let (mut images, mut requests) = (on(), Requests::default());
    let photo = |n: usize| Source::Avatar(format!("https://{}/p{n}.png", slakio_core::images::AVATAR_HOSTS[0]));
    let chips: Vec<Source> = (0..ENTRIES_MAX).map(photo).collect();
    let draw_chips = |images: &Images| {
        for c in &chips {
            images.look_as(&ws(), c, CHIP_PHOTO);
        }
    };
    for _ in 0..ENTRIES_MAX {
        draw_chips(&images);
        images.ask(&mut requests, Instant::now());
        for r in requests.take() {
            let job = images.answered(r.id, vec![1]).expect("a job");
            images.decoded(key(&job), Ok(ready(&job, 10)));
        }
    }
    assert_eq!(images.held().0, ENTRIES_MAX);
    images.take_graphics();
    // A timeline picture and one more list photo on screen with them: the picture is asked
    // first, the photo not at all (no room).
    let preview = thumb(1);
    draw_chips(&images);
    images.look_as(&ws(), &photo(999), CHIP_PHOTO);
    images.look(&ws(), &preview);
    images.ask(&mut requests, Instant::now());
    let asked = requests.take();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].command, Command::Image { source: preview.clone(), max: BYTES_MAX });
    let job = images.answered(asked[0].id, vec![1]).expect("a job");
    images.decoded(key(&job), Ok(ready(&job, 10)));
    let sent = images.take_graphics();
    assert!(sent.contains("a=d,d=I,i=1,q=2"), "the oldest list photo made room: {sent:?}");
    assert!(matches!(images.look(&ws(), &preview), Look::Ready { .. }));
    assert_eq!(images.held().0, ENTRIES_MAX);
}

#[test]
fn emoji_only_laid_out_give_the_frame_s_share_back_and_what_is_shown_keeps_it() {
    let images = on();
    let address = |n: usize| format!("https://{}/T1/e{n}/0.png", slakio_core::images::EMOJI_HOSTS[0]);
    // Shown first: e0. Then a message measured off screen looks at e0 and a share's worth more.
    images.look_emoji(&ws(), &address(0));
    let from = images.mark();
    for n in 0..=EMOJI_PER_FRAME {
        images.look_emoji(&ws(), &address(n));
    }
    images.not_shown(from, images.mark());
    // What is shown next has the share back, all but e0, which what is shown still holds.
    for n in 100..100 + EMOJI_PER_FRAME {
        images.look_emoji(&ws(), &address(n));
    }
    let wanted: Vec<Source> = images.wanted.borrow().iter().filter(|w| w.2).map(|w| w.1.0.clone()).collect();
    let mut expected: Vec<Source> =
        [0].into_iter().chain(100..99 + EMOJI_PER_FRAME).map(|n| Source::Emoji(address(n))).collect();
    expected.sort_by_key(|s| s.address().to_string());
    let mut got = wanted.clone();
    got.sort_by_key(|s| s.address().to_string());
    assert_eq!(got, expected, "the share holds for what is shown: {EMOJI_PER_FRAME}");
}

/// A diagnostics file of its own for test `name`, and what reads it.
fn diag_file(name: &str) -> (Diag, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("slakio-tui-diag-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("pictures.log");
    (Diag::to(path.clone()), path)
}

#[test]
fn with_the_diagnostics_on_each_stage_of_a_picture_has_a_line_naming_no_address() {
    let (diag, path) = diag_file("stages");
    let (mut images, mut requests) = (on(), Requests::default());
    images.set_diag(diag.clone());
    let job = fetch(&mut images, &mut requests, &thumb(1));
    images.decoded(key(&job), Ok(ready(&job, 10)));
    for _ in 0..3 {
        assert!(matches!(images.look(&ws(), &thumb(1)), Look::Ready { .. }));
    }
    let job = fetch(&mut images, &mut requests, &thumb(2));
    images.decoded(key(&job), Err(Refused::Unreadable));
    assert_eq!(images.look(&ws(), &thumb(3)), Look::Pending);
    images.ask(&mut requests, Instant::now());
    let r = requests.take().pop().expect("asked");
    assert!(images.failed(r.id, &FailKind::Network, Instant::now()));
    diag.flush();
    let text = std::fs::read_to_string(&path).unwrap();
    let id = diag.id(thumb(1).address());
    for line in [
        format!("thumb asked ui id={id} shape=preview again=false"),
        format!("thumb decoded ok id={id} shape=preview w=6 h=4 format=png bytes=10"),
        format!("thumb drawn ok id={id} shape=preview"),
        format!("thumb refused decode-unreadable id={}", diag.id(thumb(2).address())),
        format!("thumb failed network id={} shape=preview retry=60", diag.id(thumb(3).address())),
    ] {
        assert!(text.contains(&line), "no `{line}` in\n{text}");
    }
    assert_eq!(text.matches(" drawn ").count(), 1, "drawn once: {text}");
    assert!(!text.contains("files-tmb") && !text.contains("T1-F"), "{text}");
    // Off: the same flow writes nothing.
    let (mut off, mut requests) = (on(), Requests::default());
    let job = fetch(&mut off, &mut requests, &thumb(1));
    off.decoded(key(&job), Ok(ready(&job, 10)));
    diag.flush();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn with_the_diagnostics_on_a_parked_picture_says_so() {
    let (diag, path) = diag_file("parked");
    let (mut images, mut requests) = (on(), Requests::default());
    images.set_diag(diag.clone());
    let held: Vec<Source> = (0..ENTRIES_MAX).map(thumb).collect();
    for _ in 0..ENTRIES_MAX {
        frame_of(&mut images, &mut requests, &held, false);
    }
    let extra = [thumb(999)];
    frame_of(&mut images, &mut requests, &extra, true);
    let ids = frame_of(&mut images, &mut requests, &extra, true);
    let all: Vec<Source> = held.iter().chain(&extra).cloned().collect();
    frame_of(&mut images, &mut requests, &all, true);
    let job = images.answered(ids[0], vec![1]).expect("a job");
    images.decoded(key(&job), Ok(ready(&job, 10)));
    diag.flush();
    let text = std::fs::read_to_string(&path).unwrap();
    let line =
        format!("thumb parked room id={} shape=preview held={ENTRIES_MAX} bytes=640", diag.id(extra[0].address()));
    assert!(text.contains(&line), "no `{line}` in\n{text}");
}

#[test]
fn with_the_diagnostics_on_the_frame_s_emoji_share_and_the_in_flight_cap_are_told_once() {
    let (diag, path) = diag_file("skips");
    let (mut images, mut requests) = (on(), Requests::default());
    images.set_diag(diag.clone());
    let emoji = |n: usize| format!("https://emoji.example/T1/e{n}/x.png");
    for _ in 0..2 {
        for n in 0..=EMOJI_PER_FRAME {
            let _ = images.look_emoji(&ws(), &emoji(n));
        }
        images.ask(&mut requests, Instant::now());
    }
    diag.flush();
    let text = std::fs::read_to_string(&path).unwrap();
    let past = diag.id(&emoji(EMOJI_PER_FRAME));
    let share = format!("emoji skipped share id={past} shape=emoji share={EMOJI_PER_FRAME}");
    assert_eq!(text.matches(&share).count(), 1, "once: {text}");
    let fifth = text.lines().filter(|l| l.contains("emoji skipped in-flight") && l.contains("in_flight=4")).count();
    assert_eq!(fifth, 1, "the first one past the cap, once: {text}");
    assert!(!text.contains("example"), "{text}");
}
