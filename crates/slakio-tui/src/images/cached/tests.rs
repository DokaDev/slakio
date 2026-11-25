use super::*;
use crate::app::requests::Requests;
use crate::images::{CHIP_PHOTO, IN_FLIGHT_MAX, Look, PHOTO, RETRY_SOON, detect};
use slakio_core::backend::{Command, FailKind};
use slakio_core::images::{AVATAR_HOSTS, EMOJI_HOSTS, Source};
use slakio_core::model::WorkspaceId;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-tui-picture-cache-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn ws() -> WorkspaceId {
    WorkspaceId::new("T1")
}

fn photos() -> Vec<Source> {
    (0..20).map(|n| Source::Avatar(format!("https://{}/T1/U{n}-face_72.png", AVATAR_HOSTS[0]))).collect()
}

fn emoji() -> Vec<String> {
    (0..40).map(|n| format!("https://{}/T1/party{n}/0a1b2c.png", EMOJI_HOSTS[0])).collect()
}

/// A run's pictures, on, keeping pictures in `dir` (opened as a run's start does).
fn images(dir: &std::path::Path, diag: &Diag) -> Images {
    let cache = PictureCache::new(dir.to_path_buf());
    start(&cache, diag, |_| {});
    let mut images = Images { on: true, cache: Some(cache), ..Images::default() };
    images.set_diag(diag.clone());
    images
}

/// The two screens: every photo, and half the emoji each (a frame draws at most 32).
fn draw(images: &Images, screen: usize) -> usize {
    let mut ready = 0;
    for p in photos() {
        ready += usize::from(matches!(images.look(&ws(), &p), Look::Ready { .. }));
    }
    for e in &emoji()[screen * 20..screen * 20 + 20] {
        ready += usize::from(matches!(images.look_emoji(&ws(), e), Look::Ready { .. }));
    }
    ready
}

/// What the binary does with a frame's asks: each look in the cache and each fetch run as its
/// job is ([`run`]); a fetch answered with a picture, or failed offline when `offline`. The
/// pictures asked of the network.
fn settle(images: &mut Images, requests: &mut Requests, offline: bool) -> usize {
    let cache = images.cache.clone();
    let diag = images.diag().clone();
    let (mut fetched, mut quiet) = (0, 0);
    for round in 0..200 {
        let ready = draw(images, round % 2);
        images.ask(requests, Instant::now());
        let lookups = images.take_lookups();
        let asked = requests.take();
        assert!(asked.len() <= IN_FLIGHT_MAX, "the network's bound holds");
        assert!(lookups.len() <= LOOKUPS_MAX, "the bound on looks waiting holds");
        // Both screens drawn whole, and nothing asked for either: settled.
        quiet = if ready == 40 && lookups.is_empty() && asked.is_empty() { quiet + 1 } else { 0 };
        if quiet == 2 {
            return fetched;
        }
        for job in lookups {
            let result = run(&job, Some((9, 18)), cache.as_ref(), &diag);
            images.decoded((job.source, job.request), result);
        }
        for r in asked {
            let Command::Image { source, .. } = &r.command else { continue };
            fetched += 1;
            if offline {
                images.failed(r.id, &FailKind::Offline, Instant::now());
                continue;
            }
            let png = slakio_world::pictures::png(source.address(), 64, 64);
            let job = images.answered(r.id, png).expect("a job");
            let result = run(&job, Some((9, 18)), cache.as_ref(), &diag);
            images.decoded((job.source, job.request), result);
        }
    }
    panic!("never settled");
}

#[test]
fn a_second_run_draws_every_photo_and_emoji_from_the_disk_asking_nothing_even_offline() {
    let dir = scratch("second-run");
    let diag = Diag::default();
    let mut first = images(&dir, &diag);
    let fetched = settle(&mut first, &mut Requests::default(), false);
    assert_eq!(fetched, 60, "the first run fetches each once");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 60, "each kept");
    // A new run over the same folder, offline: nothing is asked of the backend (so nothing
    // counts toward the governor's picture share), and everything is drawn.
    let mut second = images(&dir, &diag);
    assert_eq!(settle(&mut second, &mut Requests::default(), true), 0);
    let mut requests = Requests::default();
    let ready = draw(&second, 0);
    second.ask(&mut requests, Instant::now());
    assert_eq!(ready + draw(&second, 1), 80, "all 20 photos and 40 emoji drawn");
    second.ask(&mut requests, Instant::now());
    assert!(requests.take().is_empty() && second.take_lookups().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_picture_on_disk_that_does_not_decode_is_removed_and_fetched_again() {
    let dir = scratch("corrupted");
    let diag = Diag::default();
    settle(&mut images(&dir, &diag), &mut Requests::default(), false);
    let cache = PictureCache::new(dir.clone());
    let victim = cache.path(&photos()[3], &shape_tag(super::super::PHOTO, Some((9, 18))));
    // A header and a PNG's first bytes, then nothing a decoder reads.
    let mut bytes = std::fs::read(&victim).unwrap();
    bytes.truncate(49 + 8);
    bytes.extend_from_slice(&[0xee; 200]);
    std::fs::write(&victim, bytes).unwrap();
    let mut second = images(&dir, &diag);
    assert_eq!(settle(&mut second, &mut Requests::default(), false), 1, "only that one is fetched");
    assert!(
        matches!(cache.load(&photos()[3], &shape_tag(super::super::PHOTO, Some((9, 18)))), Loaded::Hit(_)),
        "kept again"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_picture_removed_from_the_terminal_comes_back_from_the_disk() {
    let dir = scratch("evicted");
    let diag = Diag::default();
    let mut images = images(&dir, &diag);
    let mut requests = Requests::default();
    settle(&mut images, &mut requests, false);
    images.forget_photos();
    assert_eq!(images.held().0, 40, "the photos left the terminal");
    assert_eq!(settle(&mut images, &mut requests, false), 0, "back without a fetch");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_loop_only_hands_looks_over_never_reading_the_disk_itself() {
    let dir = scratch("off-thread");
    let cache = Some(PictureCache::new(dir.clone()));
    let mut images = Images { on: true, cache, ..Images::default() };
    let mut requests = Requests::default();
    for p in photos() {
        images.look(&ws(), &p);
    }
    assert!(images.ask(&mut requests, Instant::now()));
    assert!(requests.take().is_empty(), "nothing asked of the backend before the look");
    let lookups = images.take_lookups();
    assert_eq!(lookups.len(), photos().len(), "every photo looked for at once");
    assert!(lookups.iter().all(|j| j.from_cache && j.bytes.is_empty()));
    assert!(!dir.exists(), "the UI's side never touched the disk");
    // Without a cache (the demo, `cache = "off"`): asked of the backend as before.
    let mut plain = Images { on: true, ..Images::default() };
    plain.look(&ws(), &photos()[0]);
    plain.ask(&mut requests, Instant::now());
    assert_eq!(requests.take().len(), 1);
    assert!(plain.take_lookups().is_empty());
}

#[test]
fn the_diagnostics_tell_misses_writes_hits_and_the_sweep_by_id_only() {
    let dir = scratch("diag");
    let log = scratch("diag-log").join("pictures.log");
    let diag = Diag::to(log.clone());
    settle(&mut images(&dir, &diag), &mut Requests::default(), false);
    settle(&mut images(&dir, &diag), &mut Requests::default(), false);
    sweep(&PictureCache::new(dir.clone()), &diag);
    diag.flush();
    let text = std::fs::read_to_string(&log).unwrap();
    for want in [
        " avatar cache-miss absent id=",
        " emoji cache-miss absent id=",
        " avatar cache-stored ok id=",
        " emoji cache-hit ok id=",
        " avatar cache-hit ok id=",
        " picture-cache cache-swept ok n=0 kept=60 bytes=",
    ] {
        assert!(text.contains(want), "{want:?} in\n{text}");
    }
    for never in ["slack", "party", "https", "T1/", "U1"] {
        assert!(!text.contains(never), "{never:?} told");
    }
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(log.parent().unwrap());
}

#[test]
fn a_file_s_preview_is_never_kept_on_disk_nor_looked_for_there() {
    let dir = scratch("no-thumbs");
    let diag = Diag::default();
    let mut images = images(&dir, &diag);
    let mut requests = Requests::default();
    let thumb = Source::Thumb(format!("https://{}/files-tmb/T1-F1/x.png", slakio_core::model::FILE_HOST));
    images.look(&ws(), &thumb);
    assert!(images.ask(&mut requests, Instant::now()));
    assert!(images.take_lookups().is_empty(), "not looked for on disk");
    let r = requests.take().pop().expect("asked of the backend at once");
    assert_eq!(r.command, Command::Image { source: thumb.clone(), max: slakio_core::images::BYTES_MAX, list: false });
    let png = slakio_world::pictures::png(thumb.address(), 64, 64);
    let job = images.answered(r.id, png).expect("a job");
    let result = run(&job, Some((9, 18)), images.cache.as_ref(), &diag);
    assert!(result.is_ok(), "shown all the same");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "nothing written");
    // A preview someone else left there is never read either.
    let cache = images.cache.clone().unwrap();
    let png = slakio_world::pictures::png(thumb.address(), 64, 64);
    let d = super::decode::decode(&png, job.shape, Some((9, 18))).unwrap();
    assert!(matches!(cache.store(&thumb, &shape_tag(job.shape, Some((9, 18))), &d.png), Stored::Stored(_)));
    let look = Job { from_cache: true, bytes: Vec::new(), ..job };
    assert_eq!(run(&look, Some((9, 18)), Some(&cache), &diag), Err(Refused::NotCached));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_picture_made_for_cells_of_another_size_is_another_picture() {
    let dir = scratch("cell-size");
    let diag = Diag::default();
    let mut images = images(&dir, &diag);
    let photo = photos()[0].clone();
    images.look(&ws(), &photo);
    images.ask(&mut Requests::default(), Instant::now());
    let look = images.take_lookups().pop().expect("looked for first");
    let fetched =
        Job { from_cache: false, bytes: slakio_world::pictures::png(photo.address(), 64, 64), ..look.clone() };
    let small = run(&fetched, Some((9, 18)), images.cache.as_ref(), &diag).unwrap();
    assert!(run(&look, Some((9, 18)), images.cache.as_ref(), &diag).is_ok(), "kept for those cells");
    // The terminal's font grew: what was kept is not shown at the old size.
    assert_eq!(run(&look, Some((18, 36)), images.cache.as_ref(), &diag), Err(Refused::NotCached));
    let large = run(&fetched, Some((18, 36)), images.cache.as_ref(), &diag).unwrap();
    let again = run(&look, Some((18, 36)), images.cache.as_ref(), &diag).unwrap();
    assert_eq!((again.cols, again.rows), (large.cols, large.rows), "as a fresh decode for those cells");
    assert_eq!(again.transmit, large.transmit);
    assert!(run(&look, Some((9, 18)), images.cache.as_ref(), &diag).unwrap().transmit == small.transmit);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_fetched_picture_goes_to_the_ui_before_it_is_kept_on_disk() {
    let dir = scratch("send-first");
    let diag = Diag::default();
    let mut images = images(&dir, &diag);
    let photo = photos()[0].clone();
    images.look(&ws(), &photo);
    images.ask(&mut Requests::default(), Instant::now());
    let look = images.take_lookups().pop().expect("looked for first");
    let fetched = Job { from_cache: false, bytes: slakio_world::pictures::png(photo.address(), 64, 64), ..look };
    let files = || std::fs::read_dir(&dir).unwrap().count();
    let mut sent = None;
    run_then_keep(&fetched, Some((9, 18)), images.cache.as_ref(), &diag, |result| {
        sent = Some((result.is_ok(), files()));
    });
    assert_eq!(sent, Some((true, 0)), "handed over while nothing was written yet");
    assert_eq!(files(), 1, "kept afterwards");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_picture_on_its_way_while_the_kept_ones_are_removed_is_not_kept() {
    let dir = scratch("stale");
    let diag = Diag::default();
    let mut images = images(&dir, &diag);
    let photo = photos()[0].clone();
    images.look(&ws(), &photo);
    images.ask(&mut Requests::default(), Instant::now());
    let look = images.take_lookups().pop().expect("looked for first");
    let fetched = Job { from_cache: false, bytes: slakio_world::pictures::png(photo.address(), 64, 64), ..look };
    // Someone else signs in while it is decoded: the kept pictures go first.
    images.forget_kept();
    let empty = images.take_lookups().pop().expect("the emptying job");
    assert_eq!(run(&empty, Some((9, 18)), images.cache.as_ref(), &diag), Err(Refused::NotCached));
    let shown = run(&fetched, Some((9, 18)), images.cache.as_ref(), &diag);
    assert!(shown.is_ok(), "shown all the same");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "the old sign-in's picture is not written back");
    // A job made after the emptying is kept as before.
    let after = Job { epoch: images.cache.as_ref().unwrap().epoch(), ..fetched };
    assert!(run(&after, Some((9, 18)), images.cache.as_ref(), &diag).is_ok());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

fn thumb(n: usize) -> Source {
    Source::Thumb(format!("https://{}/files-tmb/T1-F{n}/x.png", slakio_core::model::FILE_HOST))
}

fn people(n: usize) -> Vec<Source> {
    (0..n).map(|n| Source::Avatar(format!("https://{}/T1/P{n}-face_72.png", AVATAR_HOSTS[0]))).collect()
}

/// `sources` kept in `dir` as photos for the picture slot, as a run before this one left them.
fn kept_photos(dir: &std::path::Path, sources: &[Source]) {
    let cache = PictureCache::new(dir.to_path_buf());
    cache.open().unwrap();
    for s in sources {
        let png = slakio_world::pictures::png(s.address(), 64, 64);
        assert!(matches!(cache.store(s, &shape_tag(PHOTO, Some((9, 18))), &png), Stored::Stored(_)));
    }
}

/// The looks handed over, each run against the disk and its result taken in.
fn run_looks(images: &mut Images) -> usize {
    let (cache, diag) = (images.cache.clone(), images.diag().clone());
    let lookups = images.take_lookups();
    for job in &lookups {
        let result = run(job, Some((9, 18)), cache.as_ref(), &diag);
        images.decoded((job.source.clone(), job.request), result);
    }
    lookups.len()
}

#[test]
fn forty_cached_photos_are_all_looked_up_in_the_first_frame_asking_nothing() {
    let dir = scratch("forty");
    let shown = people(40);
    kept_photos(&dir, &shown);
    let mut images = images(&dir, &Diag::default());
    let mut requests = Requests::default();
    for p in &shown {
        assert_eq!(images.look(&ws(), p), Look::Pending);
    }
    assert!(images.ask(&mut requests, Instant::now()));
    assert_eq!(run_looks(&mut images), 40, "all looked for in one frame");
    assert!(requests.take().is_empty(), "nothing asked of the network");
    let ready = shown.iter().filter(|p| matches!(images.look(&ws(), p), Look::Ready { .. })).count();
    assert_eq!(ready, 40, "all drawn on the next frame");
    images.ask(&mut requests, Instant::now());
    assert!(requests.take().is_empty() && images.take_lookups().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn disk_looks_do_not_wait_for_pictures_on_the_network() {
    let dir = scratch("not-behind");
    let shown = people(10);
    kept_photos(&dir, &shown);
    let mut images = images(&dir, &Diag::default());
    let mut requests = Requests::default();
    for n in 0..IN_FLIGHT_MAX {
        images.look(&ws(), &thumb(n));
    }
    images.ask(&mut requests, Instant::now());
    assert_eq!(requests.take().len(), IN_FLIGHT_MAX, "the network's share is out, unanswered");
    for n in 0..IN_FLIGHT_MAX {
        images.look(&ws(), &thumb(n));
    }
    for p in &shown {
        images.look(&ws(), p);
    }
    assert!(images.ask(&mut requests, Instant::now()));
    assert_eq!(run_looks(&mut images), 10, "looked for while the network is busy");
    assert!(requests.take().is_empty());
    assert!(shown.iter().all(|p| matches!(images.look(&ws(), p), Look::Ready { .. })));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn disk_looks_leave_the_network_its_four_slots() {
    let dir = scratch("four-slots");
    let shown = people(10);
    kept_photos(&dir, &shown);
    let mut images = images(&dir, &Diag::default());
    let mut requests = Requests::default();
    // The photos come first in the frame, the previews (never on disk) after them.
    for p in &shown {
        images.look(&ws(), p);
    }
    for n in 0..IN_FLIGHT_MAX {
        images.look(&ws(), &thumb(n));
    }
    images.ask(&mut requests, Instant::now());
    assert_eq!(images.take_lookups().len(), 10);
    let asked: Vec<_> = requests.take().into_iter().map(|r| r.command).collect();
    let thumbs: Vec<_> = (0..IN_FLIGHT_MAX)
        .map(|n| Command::Image { source: thumb(n), max: slakio_core::images::BYTES_MAX, list: false })
        .collect();
    assert_eq!(asked, thumbs, "the previews take all {IN_FLIGHT_MAX} slots beside the looks");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_disk_miss_is_asked_of_the_network_on_the_next_frame() {
    let dir = scratch("miss-next");
    let shown = people(6);
    let mut images = images(&dir, &Diag::default());
    let mut requests = Requests::default();
    let frame = |images: &mut Images, requests: &mut Requests| {
        for p in &shown {
            images.look(&ws(), p);
        }
        images.ask(requests, Instant::now());
        (run_looks(images), requests.take())
    };
    let (looked, asked) = frame(&mut images, &mut requests);
    assert_eq!((looked, asked.len()), (6, 0), "all looked for first, none there");
    let (looked, asked) = frame(&mut images, &mut requests);
    assert_eq!((looked, asked.len()), (0, IN_FLIGHT_MAX), "then asked, within the network's share");
    let (looked, asked) = frame(&mut images, &mut requests);
    assert_eq!((looked, asked.len()), (0, 0), "not looked for again while the others wait");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn turning_pictures_off_drops_looks_on_their_way() {
    let dir = scratch("off-drops");
    let kitty = detect::Env { term: "xterm-kitty".into(), ..detect::Env::default() };
    let mut images = Images { env: kitty, setting: "auto".into(), tty: true, ..images(&dir, &Diag::default()) };
    images.resolve();
    assert!(images.on);
    let mut requests = Requests::default();
    // A miss first: its key is noted as missed.
    images.look(&ws(), &photos()[0]);
    images.ask(&mut requests, Instant::now());
    assert_eq!(run_looks(&mut images), 1);
    assert!(images.missed.contains(&(photos()[0].clone(), PHOTO)));
    // Looks noted, not handed over yet, and the disk emptied for someone else.
    for p in &photos()[1..] {
        images.look_as(&ws(), p, CHIP_PHOTO);
    }
    images.ask(&mut requests, Instant::now());
    images.forget_kept();
    images.choose("off");
    let left = images.take_lookups();
    assert!(left.iter().all(|j| j.empty) && left.len() == 1, "only the emptying stays: {}", left.len());
    assert!(images.missed.is_empty(), "nothing missed is remembered");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_picture_tries_the_disk_before_the_network_again() {
    let dir = scratch("retry-disk");
    let mut images = images(&dir, &Diag::default());
    let mut requests = Requests::default();
    let photo = photos()[0].clone();
    let t0 = Instant::now();
    images.look(&ws(), &photo);
    images.ask(&mut requests, t0);
    assert_eq!(run_looks(&mut images), 1, "not there");
    images.look(&ws(), &photo);
    images.ask(&mut requests, t0);
    let r = requests.take().pop().expect("asked of the network");
    assert!(images.failed(r.id, &FailKind::Offline, t0));
    // Kept meanwhile (another run fetched it).
    kept_photos(&dir, std::slice::from_ref(&photo));
    let later = t0 + RETRY_SOON + Duration::from_secs(1);
    images.look(&ws(), &photo);
    assert!(images.ask(&mut requests, later));
    assert!(requests.take().is_empty(), "not asked of the network first");
    assert_eq!(run_looks(&mut images), 1, "looked for on disk again");
    assert!(matches!(images.look(&ws(), &photo), Look::Ready { .. }));
    assert_eq!(images.failures(), 0, "no longer a failure");
    let _ = std::fs::remove_dir_all(&dir);
}
