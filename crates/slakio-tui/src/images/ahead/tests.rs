use super::*;
use crate::images::cached::{LOOKUPS_MAX, run, start};
use crate::images::{Job, Look, PHOTO, RETRY_AFTER, graphics};
use slakio_core::backend::{FailKind, Generation, Request};
use slakio_core::diag::Diag;
use slakio_core::images::AVATAR_HOSTS;
use slakio_core::picture_cache::PictureCache;
use std::path::PathBuf;

const CELL: Option<(u16, u16)> = Some((9, 18));

fn ws() -> WorkspaceId {
    WorkspaceId::new("T1")
}

/// The photo of the peer of DM `n` (a row of the list, drawn or not).
fn dm(n: usize) -> Source {
    Source::Avatar(format!("https://{}/T1/D{n}-face_72.png", AVATAR_HOSTS[0]))
}

/// A timeline picture on screen: photo `n` in the comfortable layout's slot.
fn shown(n: usize) -> Key {
    (Source::Avatar(format!("https://{}/T1/S{n}-face_72.png", AVATAR_HOSTS[0])), PHOTO)
}

fn is_dm(r: &Request) -> bool {
    matches!(&r.command, Command::Image { source, .. } if source.address().contains("/D"))
}

/// A run's pictures, on, keeping pictures in a scratch folder, and the backend's requests.
struct T {
    images: Images,
    requests: Requests,
    dir: PathBuf,
    now: Instant,
    /// The picture ids of the photos ahead sent for decoding, by source.
    ids: Vec<(Source, u32)>,
}

impl Drop for T {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl T {
    fn new(name: &str) -> T {
        let dir = std::env::temp_dir().join(format!("slakio-tui-ahead-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        T::over(dir)
    }

    /// A new run over the pictures `dir` keeps.
    fn over(dir: PathBuf) -> T {
        let cache = PictureCache::new(dir.clone());
        start(&cache, &Diag::default(), |_| {});
        let images = Images { on: true, cache: Some(cache), ..Images::default() };
        T { images, requests: Requests::default(), dir, now: Instant::now(), ids: Vec::new() }
    }

    /// A frame drawing `screen` with `dms` DM rows in the list, then what it asked: the looks in
    /// the cache and the requests.
    fn frame(&mut self, screen: &[Key], dms: usize) -> (Vec<Job>, Vec<Request>) {
        for (source, shape) in screen {
            self.images.look_as(&ws(), source, *shape);
        }
        self.images.offer_ahead(true, (0..dms).map(|n| (ws(), dm(n))));
        self.images.ask(&mut self.requests, self.now);
        (self.images.take_lookups(), self.requests.take())
    }

    /// Each look run as the binary runs it, its result handed back.
    fn look(&mut self, jobs: Vec<Job>) {
        let cache = self.images.cache.clone();
        for job in jobs {
            let result = run(&job, CELL, cache.as_ref(), &Diag::default());
            self.images.decoded((job.source, job.request), result);
        }
    }

    /// Request `r` answered with a photo (kept on disk), and its decoding ended.
    fn deliver(&mut self, r: &Request) {
        let job = self.fetched(r);
        self.decode(job);
    }

    /// Request `r` answered with a photo: the job that decodes it, not run yet.
    fn fetched(&mut self, r: &Request) -> Job {
        let Command::Image { source, .. } = &r.command else { panic!("{r:?}") };
        let png = slakio_world::pictures::png(source.address(), 64, 64);
        let job = self.images.answered(r.id, png).expect("a job");
        self.ids.push((job.source.clone(), job.id));
        job
    }

    fn decode(&mut self, job: Job) {
        let cache = self.images.cache.clone();
        let result = run(&job, CELL, cache.as_ref(), &Diag::default());
        self.images.decoded((job.source, job.request), result);
    }

    /// `frames` frames drawing `screen` with `dms` DM rows, everything looked for and answered
    /// at once: the requests, in order.
    fn pump(&mut self, screen: &[Key], dms: usize, frames: usize) -> Vec<Request> {
        let mut all = Vec::new();
        for _ in 0..frames {
            let (looks, asked) = self.frame(screen, dms);
            self.look(looks);
            asked.iter().for_each(|r| self.deliver(r));
            all.extend(asked);
        }
        all
    }

    /// The photos of DM rows the terminal holds.
    fn dm_held(&self) -> usize {
        self.images.held.iter().filter(|k| k.1 == CHIP_PHOTO).count()
    }
}

#[test]
fn visible_pictures_are_asked_before_any_photo_ahead() {
    let mut t = T::new("first");
    let screen: Vec<Key> = (0..6).map(shown).collect();
    // The first frame looks on disk for what is on screen: not there.
    let (looks, asked) = t.frame(&screen, 0);
    assert_eq!((looks.len(), asked.len()), (6, 0));
    t.look(looks);
    // Four on their way, two still waiting: nothing ahead, not even a look on disk.
    let (looks, first) = t.frame(&screen, 10);
    assert_eq!(first.len(), IN_FLIGHT_MAX);
    assert!(looks.is_empty() && !first.iter().any(is_dm), "nothing ahead while two wait: {looks:?}");
    assert_eq!(t.images.ahead_waiting(), 10);
    first.iter().for_each(|r| t.deliver(r));
    let mut order = first;
    order.extend(t.pump(&screen, 10, 6));
    let last_shown = order.iter().rposition(|r| !is_dm(r)).expect("the screen's");
    let first_ahead = order.iter().position(is_dm).expect("a photo asked ahead");
    assert_eq!(order.iter().filter(|r| !is_dm(r)).count(), 6, "each on screen once");
    assert!(last_shown < first_ahead, "every picture on screen asked first: {order:?}");
}

#[test]
fn photos_ahead_fill_off_screen_dm_rows_over_time() {
    let mut t = T::new("fill");
    let mut fetched = 0;
    for _ in 0..30 {
        let asked = t.pump(&[], 10, 4);
        assert!(asked.len() <= AHEAD_MAX * 4, "one at a time");
        fetched += asked.len();
        t.now += Duration::from_secs(20);
    }
    assert_eq!(fetched, 10, "each fetched once");
    assert_eq!(t.dm_held(), 10, "every DM row's photo is in the terminal, none of them drawn");
    assert_eq!(std::fs::read_dir(&t.dir).unwrap().count(), 10, "and kept on disk");
    assert!(t.pump(&[], 10, 4).is_empty() && t.images.take_lookups().is_empty(), "nothing more");
    // Scrolled into view, each row draws its photo at once.
    for n in 0..10 {
        assert!(matches!(t.images.look_as(&ws(), &dm(n), CHIP_PHOTO), Look::Ready { .. }), "row {n}");
    }
}

#[test]
fn a_photo_ahead_only_takes_an_idle_slot() {
    let mut t = T::new("idle");
    let screen: Vec<Key> = (0..4).map(shown).collect();
    let (looks, _) = t.frame(&screen, 0);
    t.look(looks);
    let (_, slow) = t.frame(&screen, 0);
    assert_eq!(slow.len(), IN_FLIGHT_MAX, "the network's slots all taken");
    // Ahead: looked for on disk (not the network's), not there, then waits for a slot.
    let (looks, asked) = t.frame(&screen, 3);
    assert_eq!((looks.len(), asked.len()), (1, 0));
    t.look(looks);
    for _ in 0..3 {
        let (looks, asked) = t.frame(&screen, 3);
        assert!(looks.is_empty() && asked.is_empty(), "no slot idle: {asked:?}");
    }
    t.deliver(&slow[0]);
    let (_, asked) = t.frame(&screen, 3);
    assert_eq!(asked.len(), 1);
    assert!(is_dm(&asked[0]), "the idle slot");
    assert_eq!(t.images.in_flight(), IN_FLIGHT_MAX, "never more than the network's slots");
    // One ahead at once: another slot idle, still nothing more until it ends.
    t.deliver(&slow[1]);
    let (looks, more) = t.frame(&screen, 3);
    assert!(looks.is_empty() && more.is_empty(), "at most {AHEAD_MAX} ahead");
    t.deliver(&asked[0]);
    let (looks, _) = t.frame(&screen, 3);
    assert_eq!(looks.len(), 1, "the next one once it ended");
}

#[test]
fn photos_ahead_keep_to_their_minute_and_hour() {
    let mut t = T::new("share");
    let start = t.now;
    let mut minutes = Vec::new();
    // Two hours, a frame every few seconds, everything answered at once.
    while t.now < start + Duration::from_secs(2 * 3600) {
        let minute = t.pump(&[], AHEAD_QUEUE_MAX, 30).len();
        assert!(minute <= AHEAD_PER_MINUTE, "{minute} in a minute");
        minutes.push(minute);
        t.now += Duration::from_secs(60);
    }
    let first_hour: usize = minutes[..60].iter().sum();
    assert_eq!(first_hour, AHEAD_PER_HOUR, "the hour's share, and no more: {minutes:?}");
    // The minutes that spend the hour's share, a full minute's share each.
    let full = AHEAD_PER_HOUR / AHEAD_PER_MINUTE;
    assert!(full < 60 && AHEAD_PER_HOUR.is_multiple_of(AHEAD_PER_MINUTE));
    assert_eq!(
        minutes[..full],
        vec![AHEAD_PER_MINUTE; full],
        "the minute's share each minute until the hour's is spent"
    );
    assert!(minutes[full..60].iter().all(|m| *m == 0), "then none until the hour has passed");
    assert!(minutes[60..].iter().sum::<usize>() > 0, "and again after it");
}

#[test]
fn photos_ahead_leave_most_of_the_picture_share_even_when_each_is_redirected() {
    let limits = slakio_slack::governor::Limits::default();
    // Every hop counts: a photo and each redirect it may follow.
    let hops = slakio_slack::picture::AVATAR_REDIRECTS + 1;
    let (minute, hour) = (AHEAD_PER_MINUTE * hops, AHEAD_PER_HOUR * hops);
    // At most two fifths of a workspace's picture share, so what is on screen keeps the rest.
    assert!(minute * 5 <= limits.lane_picture_per_minute as usize * 2, "{minute} hops a minute");
    assert!(hour * 5 <= limits.lane_picture_per_hour as usize * 2, "{hour} hops an hour");
}

#[test]
fn a_photo_ahead_waits_while_the_looks_on_disk_are_at_their_most() {
    let mut t = T::new("looks");
    let screen: Vec<Key> = (0..LOOKUPS_MAX).map(shown).collect();
    let (mut looks, _) = t.frame(&screen, 0);
    assert_eq!(looks.len(), LOOKUPS_MAX);
    for _ in 0..3 {
        let (more, _) = t.frame(&screen, 3);
        assert!(more.is_empty(), "no look ahead past {LOOKUPS_MAX} waiting: {}", more.len());
    }
    // One ends: there is room for one look, and the photo ahead takes it.
    t.look(vec![looks.remove(0)]);
    let (more, _) = t.frame(&screen, 3);
    assert_eq!(more.len(), 1);
    assert_eq!(more[0].source, dm(0));
}

#[test]
fn a_refused_picture_pauses_photos_ahead() {
    for kind in [FailKind::Busy, FailKind::CapReached, FailKind::Offline, FailKind::RateLimited { retry_after: None }] {
        let mut t = T::new(&format!("pause-{kind:?}"));
        let screen = [shown(0)];
        let (looks, _) = t.frame(&screen, 0);
        t.look(looks);
        let (_, asked) = t.frame(&screen, 0);
        t.images.failed(asked[0].id, &kind, t.now);
        // Ahead: looked for on disk, not there; the network is not asked while it is paused.
        let start = t.now;
        let (looks, _) = t.frame(&[], 3);
        t.look(looks);
        while t.now < start + RETRY_AFTER {
            assert!(t.pump(&[], 3, 2).is_empty(), "{kind:?}: paused");
            t.now += Duration::from_secs(5);
        }
        t.now = start + RETRY_AFTER;
        let asked = t.pump(&[], 3, 2);
        assert!(!asked.is_empty() && asked.iter().all(is_dm), "{kind:?}: again once the pause is over");
    }
}

/// A run with photo `0` of the screen failed for `kind` at its `now`, and three DM rows' photos
/// looked for on disk and not there (waiting for the network).
fn refused(name: &str, kind: &FailKind) -> T {
    let mut t = T::new(name);
    let screen = [shown(0)];
    let (looks, _) = t.frame(&screen, 0);
    t.look(looks);
    let (_, asked) = t.frame(&screen, 0);
    t.images.failed(asked[0].id, kind, t.now);
    let (looks, _) = t.frame(&[], 3);
    t.look(looks);
    t
}

/// Nothing is asked ahead from `t.now` until `until`, and something is at `until`.
fn paused_until(t: &mut T, until: Instant, what: &str) {
    while t.now < until {
        assert!(t.pump(&[], 3, 2).is_empty(), "{what}: paused");
        t.now = (t.now + Duration::from_secs(5)).min(until);
    }
    assert!(!t.pump(&[], 3, 2).is_empty(), "{what}: again once the pause is over");
}

#[test]
fn a_429_pauses_photos_ahead_as_long_as_it_says_and_a_shorter_pause_never_shortens_it() {
    let long = Duration::from_secs(300);
    let mut t = refused("429-long", &FailKind::RateLimited { retry_after: Some(long) });
    let start = t.now;
    // Busy a little later: a shorter pause, which leaves the longer one as it is.
    t.now += Duration::from_secs(30);
    t.images.failed(Generation(u64::MAX / 2), &FailKind::Busy, t.now);
    paused_until(&mut t, start + long, "the 429's five minutes");
    // A 429 asking for less than the pause slakio keeps waits that long all the same.
    let mut t = refused("429-short", &FailKind::RateLimited { retry_after: Some(Duration::from_secs(1)) });
    let start = t.now;
    paused_until(&mut t, start + RETRY_AFTER, "at least a minute");
}

#[test]
fn a_refusal_for_a_picture_no_longer_waited_for_still_pauses_photos_ahead() {
    let mut t = T::new("pause-dropped");
    let (looks, _) = t.frame(&[], 3);
    t.look(looks);
    // Busy for a request not waited for any more (forgotten when photos were turned off): the
    // backend is busy all the same.
    assert!(!t.images.failed(Generation(u64::MAX / 2), &FailKind::Busy, t.now));
    let start = t.now;
    paused_until(&mut t, start + RETRY_AFTER, "busy");
}

/// A run whose only DM row's photo, asked for ahead, failed for `kind`; then two hours of a
/// frame every ten seconds, everything else answered at once: the requests for it after that,
/// each with how long after the failure it was asked.
fn after_failing(kind: &FailKind) -> Vec<(Request, Duration)> {
    let mut t = T::new(&format!("failed-{kind:?}").replace(['{', '}', ' ', ':', '"'], ""));
    let (looks, _) = t.frame(&[], 1);
    t.look(looks);
    let (_, asked) = t.frame(&[], 1);
    assert!(asked.len() == 1 && is_dm(&asked[0]));
    t.images.failed(asked[0].id, kind, t.now);
    let failed = t.now;
    let mut again = Vec::new();
    for _ in 0..720 {
        let since = t.now - failed;
        again.extend(t.pump(&[], 1, 1).into_iter().map(|r| (r, since)));
        t.now += Duration::from_secs(10);
    }
    again
}

#[test]
fn a_photo_ahead_that_failed_for_a_while_is_asked_again_once_its_wait_is_over() {
    let passing = [
        FailKind::Network,
        FailKind::Busy,
        FailKind::Offline,
        FailKind::CapReached,
        FailKind::RateLimited { retry_after: None },
    ];
    for kind in passing {
        let again = after_failing(&kind);
        assert_eq!(again.len(), 1, "{kind:?}: asked again, once");
        // Its wait (or the pause) is the minute a failure for a while waits, at least.
        assert!(again[0].1 >= RETRY_AFTER, "{kind:?}: asked again after {:?}", again[0].1);
    }
}

#[test]
fn a_photo_ahead_that_cannot_be_shown_is_not_asked_again() {
    let lasting = [
        FailKind::Api { code: "not_an_image".into() },
        FailKind::Api { code: "file_not_found".into() },
        FailKind::TooLarge,
        FailKind::Auth,
    ];
    for kind in lasting {
        assert!(after_failing(&kind).is_empty(), "{kind:?}: not asked again");
    }
}

#[test]
fn a_photo_ahead_on_disk_is_not_fetched() {
    let mut first = T::new("kept");
    for _ in 0..4 {
        first.pump(&[], 5, 4);
        first.now += Duration::from_secs(61);
    }
    assert_eq!(first.dm_held(), 5);
    // The folder stays for the next run, which removes it.
    let dir = std::mem::take(&mut first.dir);
    drop(first);
    // The next run: every photo ahead read from the disk, none asked of the network.
    let mut next = T::over(dir);
    let mut looks = 0;
    for _ in 0..20 {
        let (jobs, asked) = next.frame(&[], 5);
        assert!(asked.is_empty(), "nothing fetched: {asked:?}");
        looks += jobs.len();
        next.look(jobs);
    }
    assert_eq!(looks, 5, "each looked for once");
    assert_eq!(next.dm_held(), 5, "and sent to the terminal");
}

/// A run whose terminal holds `ENTRIES_MAX - 1` timeline pictures, all on screen, with one more
/// on screen on its way; and a photo ahead fetched, not decoded yet.
fn nearly_full(name: &str) -> (T, Vec<Key>, Request, Job) {
    let mut t = T::new(name);
    let mut screen: Vec<Key> = (0..ENTRIES_MAX - 1).map(shown).collect();
    for _ in 0..40 {
        t.pump(&screen, 0, 1);
    }
    assert_eq!(t.images.held().0, ENTRIES_MAX - 1);
    screen.push(shown(ENTRIES_MAX - 1));
    let (looks, _) = t.frame(&screen, 0);
    t.look(looks);
    let (_, last) = t.frame(&screen, 0);
    assert_eq!(last.len(), 1);
    // Ahead: not on disk, then fetched.
    let (looks, _) = t.frame(&screen, 1);
    t.look(looks);
    let (_, asked) = t.frame(&screen, 1);
    assert!(asked.len() == 1 && is_dm(&asked[0]));
    let job = t.fetched(&asked[0]);
    (t, screen, last[0].clone(), job)
}

#[test]
fn a_photo_ahead_never_takes_the_room_of_one_on_screen() {
    let (mut t, mut screen, last, job) = nearly_full("room");
    let id = job.id;
    t.images.take_graphics();
    t.decode(job);
    t.deliver(&last);
    t.frame(&screen, 1);
    let sent = t.images.take_graphics();
    assert!(!sent.contains(&format!("a=T,U=1,f=100,t=d,i={id},")), "the photo ahead was never sent");
    assert!(!sent.contains("a=d"), "nothing on screen removed");
    assert!(matches!(t.images.look_as(&ws(), &shown(ENTRIES_MAX - 1).0, PHOTO), Look::Ready { .. }));
    assert_eq!(t.images.held().0, ENTRIES_MAX);
    assert!(std::fs::read_dir(&t.dir).unwrap().count() > ENTRIES_MAX, "the photo ahead is on disk");
    // Ten of them scrolled away: the next photo ahead removes none of them either.
    screen.drain(..10);
    t.pump(&screen, 1, 2);
    let ahead = t.pump(&screen, 2, 4);
    assert_eq!(ahead.len(), 1, "the second photo ahead fetched");
    assert!(!t.images.take_graphics().contains("a=d"), "nothing removed for a photo ahead");
    assert_eq!(t.dm_held(), 0);
}

#[test]
fn a_photo_ahead_not_on_screen_is_the_first_to_leave_the_terminal() {
    let mut t = T::new("oldest");
    let screen: Vec<Key> = (0..ENTRIES_MAX - 2).map(shown).collect();
    for _ in 0..40 {
        t.pump(&screen, 0, 1);
    }
    assert_eq!(t.images.held().0, ENTRIES_MAX - 2);
    // Ten seen and scrolled away, then a photo ahead: there is room for it.
    let mut screen = screen[10..].to_vec();
    t.pump(&screen, 0, 2);
    assert_eq!(t.pump(&screen, 1, 3).len(), 1);
    assert_eq!(t.dm_held(), 1);
    let (_, id) = t.ids.iter().find(|(s, _)| *s == dm(0)).cloned().expect("the photo ahead");
    t.images.take_graphics();
    // Two more on screen: the terminal is full, so one picture goes, the photo ahead first.
    screen.extend([shown(ENTRIES_MAX - 2), shown(ENTRIES_MAX - 1)]);
    t.pump(&screen, 1, 4);
    let sent = t.images.take_graphics();
    assert_eq!(sent.matches("a=d,d=I").count(), 1, "one picture removed");
    assert!(sent.contains(&graphics::delete(id)), "the photo ahead, before any picture seen");
    assert_eq!(t.dm_held(), 0);
    assert!(matches!(t.images.look_as(&ws(), &shown(0).0, PHOTO), Look::Ready { .. }), "the oldest seen stays");
}

#[test]
fn a_row_drawn_while_its_photo_is_ahead_shows_it() {
    let (mut t, mut screen, last, job) = nearly_full("drawn");
    t.deliver(&last);
    // The terminal is full; the row scrolls into view while its photo is on its way.
    screen.drain(..10);
    let row = (dm(0), CHIP_PHOTO);
    screen.push(row.clone());
    assert_eq!(t.images.look_as(&ws(), &row.0, CHIP_PHOTO), Look::Pending);
    // Two frames: the rows scrolled away are no longer on screen.
    t.frame(&screen, 1);
    t.frame(&screen, 1);
    t.decode(job);
    assert!(matches!(t.images.look_as(&ws(), &row.0, CHIP_PHOTO), Look::Ready { .. }), "drawn at once");
}

#[test]
fn nothing_is_fetched_ahead_with_pictures_or_photos_off() {
    let mut t = T::new("off");
    t.images.offer_ahead(true, (0..5).map(|n| (ws(), dm(n))));
    assert_eq!(t.images.ahead_waiting(), 5, "on: waiting");
    // Photos (or avatars) off: none waits, nothing is asked.
    t.images.offer_ahead(false, (0..5).map(|n| (ws(), dm(n))));
    assert_eq!(t.images.ahead_waiting(), 0);
    assert!(!t.images.ask(&mut t.requests, t.now));
    assert!(t.images.take_lookups().is_empty() && t.requests.take().is_empty());
    // The demo, and no cache on disk: none either.
    t.images.demo = true;
    t.images.offer_ahead(true, (0..5).map(|n| (ws(), dm(n))));
    assert_eq!(t.images.ahead_waiting(), 0, "demo");
    t.images.demo = false;
    let cache = t.images.cache.take();
    t.images.offer_ahead(true, (0..5).map(|n| (ws(), dm(n))));
    assert_eq!(t.images.ahead_waiting(), 0, "no cache");
    t.images.cache = cache;
    // On its way when photos are turned off: the queue goes, the look's result is dropped.
    let (looks, _) = t.frame(&[], 5);
    assert_eq!(looks.len(), 1);
    assert_eq!(t.images.ahead_waiting(), 4);
    t.images.forget_photos();
    assert_eq!(t.images.ahead_waiting(), 0, "forgotten with the photos");
    // Pictures turned off: the same, and nothing is asked while they are.
    t.frame(&[], 5);
    t.images.resolve();
    assert!(!t.images.on);
    assert_eq!(t.images.ahead_waiting(), 0, "forgotten with the pictures");
    t.look(looks);
    for _ in 0..5 {
        let (looks, asked) = t.frame(&[], 5);
        assert!(looks.is_empty() && asked.is_empty());
    }
    assert_eq!(t.images.held().0, 0);
}

#[test]
fn photos_are_wanted_ahead_only_with_pictures_photos_a_cache_and_not_in_the_demo() {
    let t = T::new("wants");
    let on = Images { on: true, cache: t.images.cache.clone(), ..Images::default() };
    assert!(on.wants_ahead(true));
    assert!(!on.wants_ahead(false), "photos or avatars off");
    assert!(!Images { on: false, cache: on.cache.clone(), ..Images::default() }.wants_ahead(true), "pictures off");
    assert!(!Images { on: true, ..Images::default() }.wants_ahead(true), "no cache on disk");
    assert!(!Images { on: true, demo: true, cache: on.cache.clone(), ..Images::default() }.wants_ahead(true), "demo");
}

#[test]
fn the_photos_done_ahead_are_kept_within_a_bound() {
    let mut t = T::new("done-bound");
    for n in 0..3 * crate::images::FAILED_MAX {
        t.images.done_ahead((dm(n), CHIP_PHOTO));
        assert!(t.images.ahead.done.len() <= crate::images::FAILED_MAX, "{} kept", t.images.ahead.done.len());
    }
    assert!(t.images.ahead.done.contains(&(dm(3 * crate::images::FAILED_MAX - 1), CHIP_PHOTO)), "the latest kept");
}
