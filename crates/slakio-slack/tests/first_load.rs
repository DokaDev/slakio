//! The first load's pictures against the fake Slack, on a fake clock: a screen of 20 people's
//! photos and 40 of the workspace's own emoji, asked as the UI asks them (at most four at once).
//! Each is fetched at once, with no wait of its own between asks, while the workspace's share of
//! pictures has room; the rest wait for the share's minute, then come.

use slakio_core::backend::{BackendHandle, Command, Envelope, Event, FailKind, Generation, Request};
use slakio_core::images::{AVATAR_HOSTS, BYTES_MAX, Source};
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, PICTURES};
use slakio_slack::Client;
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_world::World;
use std::sync::Arc;
use std::time::Duration;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

/// Ask `sources` four at a time, as the UI does; those that failed, by kind.
async fn ask_all(handle: &mut BackendHandle, id: &mut u64, sources: &[Source]) -> Vec<(Source, FailKind)> {
    let mut failed = Vec::new();
    for four in sources.chunks(4) {
        for source in four {
            *id += 1;
            let command = Command::Image { source: source.clone(), max: BYTES_MAX };
            handle.send(Request { id: Generation(*id), workspace: None, command }).unwrap();
        }
        for _ in four {
            let e: Envelope =
                tokio::time::timeout(Duration::from_secs(20), handle.rx.recv()).await.expect("an answer").unwrap();
            let at = usize::try_from(e.generation.0 - (*id - four.len() as u64) - 1).unwrap();
            match e.event {
                Event::Image { .. } => {}
                Event::Failed { kind, .. } => failed.push((four[at].clone(), kind)),
                other => panic!("{other:?}"),
            }
        }
    }
    failed
}

#[test]
fn twenty_photos_and_forty_emoji_come_at_once_within_the_picture_share_and_the_rest_a_minute_later() {
    rt().block_on(async {
        let world = World::demo();
        let team = world.snapshot().workspaces[0].id.as_str().to_string();
        let fake = FakeSlack::start(World::demo()).await.unwrap();
        let clock = FakeClock::default();
        let limits = Limits::default();
        let governor = Governor::new(limits, Box::new(clock.clone()), true);
        let client = Client::new(
            Credentials::new(concat!("xo", "xc-TEST"), "cookie-TEST"),
            Endpoints::local(fake.addr()).unwrap(),
            Arc::clone(&governor),
        );
        let mut handle = slakio_slack::serve(client);
        handle.send(Request { id: Generation(1), workspace: None, command: Command::Boot }).unwrap();
        let booted = tokio::time::timeout(Duration::from_secs(20), handle.rx.recv()).await.unwrap().unwrap();
        assert!(matches!(booted.event, Event::Booted(_)));
        let photos = (0..20).map(|i| Source::Avatar(format!("https://{}/avatars/T/U{i:02}-72.png", AVATAR_HOSTS[0])));
        let emoji = (0..40).map(|i| Source::Emoji(slakio_world::emoji::picture(&team, &format!("e{i:02}"))));
        let sources: Vec<Source> = photos.chain(emoji).collect();
        let mut id = 1;
        let started = std::time::Instant::now();
        let failed = ask_all(&mut handle, &mut id, &sources).await;
        let share = usize::try_from(limits.lane_picture_per_minute).unwrap();
        assert_eq!(sources.len() - failed.len(), share, "the share's worth at once: {failed:?}");
        assert!(failed.iter().all(|(_, k)| *k == FailKind::CapReached), "{failed:?}");
        assert!(started.elapsed() < Duration::from_secs(10), "no wait between asks: {:?}", started.elapsed());
        // Asked in order: the first ones on screen came, the last waited.
        assert_eq!(failed[0].0, sources[share], "the 31st waits, not one before it");
        // The share's minute later, the rest come.
        clock.advance(Duration::from_secs(60));
        let rest: Vec<Source> = failed.into_iter().map(|(s, _)| s).collect();
        assert!(ask_all(&mut handle, &mut id, &rest).await.is_empty(), "the rest come a minute later");
        let fetched = fake.log().into_iter().filter(|l| l.method() == PICTURES).count();
        assert_eq!(fetched, sources.len(), "each fetched once: nothing asked twice, nothing past the share");
    });
}
