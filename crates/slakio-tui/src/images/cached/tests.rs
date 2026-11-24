use super::*;
use crate::app::requests::Requests;
use crate::images::{IN_FLIGHT_MAX, Look};
use slakio_core::backend::{Command, FailKind};
use slakio_core::images::{AVATAR_HOSTS, EMOJI_HOSTS, Source};
use slakio_core::model::WorkspaceId;
use std::path::PathBuf;
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
        assert!(lookups.len() + asked.len() <= IN_FLIGHT_MAX, "the bound on what is on its way holds");
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
    assert_eq!(lookups.len(), IN_FLIGHT_MAX, "looks count among what is on its way");
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
    assert_eq!(r.command, Command::Image { source: thumb.clone(), max: slakio_core::images::BYTES_MAX });
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
