//! The pace of pictures against the fake Slack, on a clock the test moves: a screen's photos and
//! emoji all come within a few virtual seconds, inside the picture hosts' budget and never in the
//! API's shares (the socket's openings and the lookups keep their room); a host that answers 429
//! is paused alone; the file previews keep to the pictures' share; nothing of the session ever
//! reaches a picture host; the picture hosts' run alarm is told once.

use slakio_core::backend::{BackendHandle, Command, Envelope, Event, FailKind, Generation, Request};
use slakio_core::diag::Diag;
use slakio_core::images::{BYTES_MAX, Source};
use slakio_core::model::WorkspaceId;
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault, PICTURES};
use slakio_slack::governor::{CdnLimits, FakeClock, Governor, Limits, Priority};
use slakio_slack::request::Endpoints;
use slakio_slack::{Client, Method};
use slakio_world::World;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const TOKEN: &str = concat!("xo", "xc-TEST");
const COOKIE: &str = "cookie-TEST";
const TEAM: &str = "T1";
/// One round of asking: the virtual time it takes.
const ROUND: Duration = Duration::from_millis(100);
/// The most asked in one round (the handle's channel holds a few more).
const BATCH: usize = 24;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

struct Run {
    fake: FakeSlack,
    handle: BackendHandle,
    governor: Arc<Governor>,
    clock: FakeClock,
    id: u64,
}

impl Run {
    async fn with(cdn: Option<CdnLimits>, diag: Diag) -> Self {
        let fake = FakeSlack::start(World::demo()).await.unwrap();
        let clock = FakeClock::default();
        let governor = match cdn {
            Some(cdn) => Governor::with_cdn(Limits::default(), cdn, Box::new(clock.clone()), true),
            None => Governor::new(Limits::default(), Box::new(clock.clone()), true),
        };
        let endpoints = Endpoints::local(fake.addr()).unwrap();
        let client =
            Client::of_team(Credentials::new(TOKEN, COOKIE), endpoints, Arc::clone(&governor), TEAM).with_diag(diag);
        Self { fake, handle: slakio_slack::serve(client), governor, clock, id: 0 }
    }

    async fn new() -> Self {
        Self::with(None, Diag::default()).await
    }

    fn send(&mut self, source: &Source, list: bool) -> Generation {
        self.id += 1;
        let command = Command::Image { source: source.clone(), max: BYTES_MAX, list };
        self.handle.send(Request { id: Generation(self.id), workspace: None, command }).unwrap();
        Generation(self.id)
    }

    async fn next(&mut self) -> Envelope {
        tokio::time::timeout(Duration::from_secs(20), self.handle.rx.recv()).await.expect("an answer").unwrap()
    }

    async fn picture(&mut self, source: &Source) -> Result<Vec<u8>, FailKind> {
        let id = self.send(source, false);
        let e = self.next().await;
        assert_eq!(e.generation, id);
        match e.event {
            Event::Image { bytes } => Ok(bytes),
            Event::Failed { kind, .. } => Err(kind),
            other => panic!("{other:?}"),
        }
    }

    /// Every one of `sources` asked, [`BATCH`] a round, a busy answer asked again in a later
    /// round (each round [`ROUND`] of virtual time): the virtual time all took, or the first refusal that is
    /// not busy.
    async fn all(&mut self, sources: &[Source]) -> Result<Duration, (usize, FailKind)> {
        let start = self.clock_now();
        let mut waiting: Vec<usize> = (0..sources.len()).collect();
        let mut got = 0;
        while !waiting.is_empty() {
            let batch: Vec<usize> = waiting.drain(..waiting.len().min(BATCH)).collect();
            let asked: HashMap<Generation, usize> =
                batch.into_iter().map(|i| (self.send(&sources[i], false), i)).collect();
            for _ in 0..asked.len() {
                let e = self.next().await;
                let i = asked[&e.generation];
                match e.event {
                    Event::Image { .. } => got += 1,
                    Event::Failed { kind: FailKind::Busy, .. } => waiting.push(i),
                    Event::Failed { kind, .. } => return Err((got, kind)),
                    other => panic!("{other:?}"),
                }
            }
            self.clock.advance(ROUND);
        }
        Ok(self.clock_now() - start)
    }

    fn clock_now(&self) -> Duration {
        use slakio_slack::governor::Clock;
        self.clock.now()
    }

    fn picture_requests(&self) -> Vec<slakio_fake::Logged> {
        self.fake.log().into_iter().filter(|l| l.method() == PICTURES).collect()
    }
}

/// `n` distinct profile photos on the avatar host.
fn photos(n: usize) -> Vec<Source> {
    let ws = WorkspaceId::new(TEAM);
    (0..)
        .filter_map(|i| slakio_world::pictures::avatar(&ws, &format!("U{i:04}"), false))
        .take(n)
        .map(Source::Avatar)
        .collect()
}

/// `n` distinct emoji of the workspace on the emoji host.
fn emoji(n: usize) -> Vec<Source> {
    (0..n).map(|i| Source::Emoji(slakio_world::emoji::picture(TEAM, &format!("e{i}")))).collect()
}

fn thumb() -> Source {
    let w = World::demo();
    let s = w.snapshot();
    let c = s.conversations.iter().find(|c| c.name == "design-review").unwrap();
    let thumb = (0..w.message_count(&c.id)).flat_map(|i| w.message(&c.id, i).unwrap().files).find_map(|f| f.thumb);
    Source::Thumb(thumb.expect("a picture file"))
}

fn header<'a>(l: &'a slakio_fake::Logged, name: &str) -> Option<&'a str> {
    l.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// 40 photos and 60 emoji, as a busy channel's screens show them.
fn screenful() -> Vec<Source> {
    let mut all = photos(40);
    all.extend(emoji(60));
    all
}

#[test]
fn forty_photos_and_sixty_emoji_all_come_within_three_virtual_seconds() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let took = r.all(&screenful()).await.expect("none refused");
        assert!(took <= Duration::from_secs(3), "took {took:?}");
        assert_eq!(r.picture_requests().len(), 100, "each asked once");
    });
}

#[test]
fn photos_and_emoji_leave_the_socket_and_lookups_their_room() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let mut sources = photos(50);
        sources.extend(emoji(70));
        r.all(&sources).await.expect("120 photos and emoji in a minute");
        assert_eq!(r.governor.sent(), 0, "none of them in the API's count");
        // The workspace's whole background share of the minute is still there.
        let lane = r.governor.lane_of(TEAM);
        let share = Limits::default().lane_background_per_minute;
        for _ in 0..share - 1 {
            r.governor.dial(&lane).expect("the socket opens");
        }
        drop(r.governor.admit_in(&lane, Method::UsersInfo, Priority::Background).await.expect("a lookup goes"));
    });
}

#[test]
fn a_429_from_the_emoji_host_pauses_that_host_only() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let (e, p) = (emoji(2), photos(1));
        let limited = Fault::Status { status: 429, retry_after: Some("5".to_string()) };
        r.fake.fail_next_picture("emoji", limited);
        assert_eq!(r.picture(&e[0]).await, Err(FailKind::RateLimited { retry_after: Some(Duration::from_secs(5)) }));
        let asked = r.picture_requests().len();
        assert_eq!(r.picture(&e[1]).await, Err(FailKind::RateLimited { retry_after: Some(Duration::from_secs(5)) }));
        assert_eq!(r.picture_requests().len(), asked, "the paused host is not asked");
        r.picture(&p[0]).await.expect("the avatar host still goes");
        r.clock.advance(Duration::from_secs(5));
        r.picture(&e[1]).await.expect("after its pause");
    });
}

#[test]
fn previews_keep_to_the_picture_share() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let t = thumb();
        let share = Limits::default().lane_picture_per_minute;
        for _ in 0..share {
            r.picture(&t).await.expect("within the share");
        }
        assert_eq!(r.picture(&t).await, Err(FailKind::CapReached), "the 31st preview of a minute");
    });
}

#[test]
fn no_cookie_or_token_ever_reaches_the_picture_hosts_at_speed() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.all(&screenful()).await.expect("none refused");
        let log = r.picture_requests();
        assert_eq!(log.len(), 100);
        for l in &log {
            for name in ["cookie", "authorization", "origin", "referer"] {
                assert_eq!(header(l, name), None, "{name} on {}", l.path);
            }
            assert!(l.headers.iter().all(|(_, v)| !v.contains(TOKEN) && !v.contains(COOKIE)), "nothing of the session");
            assert!(l.body.is_empty(), "a GET");
        }
    });
}

#[test]
fn the_cdn_run_alarm_logs_once() {
    rt().block_on(async {
        let dir = std::env::temp_dir().join(format!("slakio-pace-it-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("state").join("pictures.log");
        let diag = Diag::to(path.clone());
        let cdn = CdnLimits { per_run: 3, ..CdnLimits::default() };
        let mut r = Run::with(Some(cdn), diag.clone()).await;
        let sources = screenful();
        for s in &sources[..3] {
            r.picture(s).await.expect("within the run");
        }
        for s in &sources[3..10] {
            assert_eq!(r.picture(s).await, Err(FailKind::RunLimit), "the run's limit, for the rest of the run");
        }
        assert!(r.governor.is_on(), "the network stays on");
        diag.flush();
        let text = std::fs::read_to_string(&path).unwrap();
        let alarms: Vec<&str> = text.lines().filter(|l| l.contains("run-limit")).collect();
        assert_eq!(alarms.len(), 1, "{text}");
        assert!(alarms[0].contains("cap-reached"), "{}", alarms[0]);
        assert_eq!(text.lines().filter(|l| l.contains("cap-reached")).count(), 1, "nothing more of it: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    });
}

/// The previews (asked with the session's cookie) keep to four on their way at once in the
/// backend too, whatever the UI asks: a fifth is answered busy at once, and a photo still goes.
#[test]
fn previews_keep_to_four_at_once_in_the_backend() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let host = slakio_core::model::FILE_HOST;
        let thumbs: Vec<Source> =
            (0..5).map(|i| Source::Thumb(format!("https://{host}/files-tmb/T1-F{i}/shot_360.png"))).collect();
        // Four that never finish coming (until their deadline).
        for _ in 0..4 {
            r.fake.fail_next_picture("thumb", Fault::Partial { length: 1000, body: vec![0x89; 8] });
        }
        for t in &thumbs[..4] {
            r.send(t, false);
        }
        let asked = |r: &Run| r.fake.log().iter().filter(|l| l.picture_kind() == Some("thumb")).count();
        let start = std::time::Instant::now();
        while asked(&r) < 4 {
            assert!(start.elapsed() < Duration::from_secs(10), "the four never went");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let fifth = r.send(&thumbs[4], false);
        let e = r.next().await;
        assert_eq!(e.generation, fifth);
        let busy = matches!(e.event, Event::Failed { kind: FailKind::Busy, .. });
        assert!(
            busy,
            "the fifth preview is answered busy, not {}",
            if matches!(e.event, Event::Image { .. }) { "fetched" } else { "failed otherwise" }
        );
        let photo = Source::Avatar(World::demo().snapshot().users.iter().find_map(|u| u.avatar.clone()).unwrap());
        assert!(r.picture(&photo).await.is_ok(), "a photo is no preview");
        assert_eq!(asked(&r), 4, "the fifth preview was never sent");
    });
}

/// A photo waiting for one of the picture hosts' places does not spend its own deadline
/// waiting: once the place is free it is fetched, never failed as too slow for the wait. (Real
/// time: the photo before it holds the only place until its deadline, about 25 seconds.)
#[test]
fn a_photo_waiting_for_a_place_is_not_failed_by_the_wait() {
    rt().block_on(async {
        let cdn = CdnLimits { in_flight: 1, ..CdnLimits::default() };
        let mut r = Run::with(Some(cdn), Diag::default()).await;
        let photos: Vec<Source> = World::demo()
            .snapshot()
            .users
            .iter()
            .filter_map(|u| u.avatar.clone())
            .take(2)
            .map(Source::Avatar)
            .collect();
        r.fake.fail_next_picture("avatar", Fault::Partial { length: 1000, body: vec![0x89; 8] });
        let slow = r.send(&photos[0], false);
        let waiting = r.send(&photos[1], false);
        let mut got = HashMap::new();
        for _ in 0..2 {
            let e =
                tokio::time::timeout(Duration::from_secs(60), r.handle.rx.recv()).await.expect("an answer").unwrap();
            got.insert(e.generation, e.event);
        }
        assert!(matches!(got[&slow], Event::Failed { kind: FailKind::Network, .. }), "the first ran out of time");
        assert!(
            matches!(got[&waiting], Event::Image { .. }),
            "the second came once the place was free: {:?}",
            matches!(got[&waiting], Event::Failed { .. })
        );
    });
}
