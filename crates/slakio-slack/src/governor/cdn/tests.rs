use super::super::{FakeClock, Limits, Priority, WINDOW};
use super::*;
use crate::method::Method;

const AVATARS: &str = "avatars.example";
const EMOJI: &str = "emoji.example";

fn governor(limits: Limits, cdn: CdnLimits) -> (Arc<Governor>, FakeClock) {
    let clock = FakeClock::default();
    (Governor::with_cdn(limits, cdn, Box::new(clock.clone()), true), clock)
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

fn cap(r: Result<(), SlackError>) -> bool {
    matches!(r, Err(SlackError::CapReached { .. }))
}

#[test]
fn the_cdn_takes_three_hundred_a_minute_and_three_thousand_an_hour() {
    assert_eq!(CdnLimits::default(), CdnLimits { per_minute: 300, per_hour: 3000, in_flight: 12, per_run: 20_000 });
    let (g, clock) = governor(Limits::default(), CdnLimits::default());
    for _ in 0..300 {
        g.cdn(AVATARS, false).unwrap();
    }
    assert!(cap(g.cdn(EMOJI, false)), "the 301st of a minute");
    assert!(g.is_on(), "a full minute does not switch the network off");
    for _ in 0..9 {
        clock.advance(WINDOW);
        for _ in 0..300 {
            g.cdn(EMOJI, false).unwrap();
        }
    }
    clock.advance(WINDOW);
    assert!(cap(g.cdn(AVATARS, false)), "3000 in the hour");
    clock.advance(HOUR - WINDOW * 9);
    g.cdn(AVATARS, false).expect("the oldest left the hour");
}

#[test]
fn list_pictures_keep_to_two_thirds_of_the_cdn() {
    let (g, _) = governor(Limits::default(), CdnLimits::default());
    for _ in 0..200 {
        g.cdn(AVATARS, true).unwrap();
    }
    assert!(cap(g.cdn(AVATARS, true)), "the 201st list photo of a minute");
    g.cdn(AVATARS, false).expect("the timeline still goes");
}

#[test]
fn cdn_fetches_never_count_in_the_api_shares() {
    rt().block_on(async {
        let (g, _) = governor(Limits::default(), CdnLimits::default());
        let lane = g.lane();
        for _ in 0..300 {
            g.cdn(AVATARS, false).unwrap();
        }
        for _ in 0..60 {
            g.dial(&lane).expect("the socket's opening keeps its room");
        }
        let lane = g.lane();
        drop(g.admit_in(&lane, Method::ClientCounts, Priority::Background).await.expect("a lookup keeps its room"));
        g.picture(&lane).expect("the previews' share is untouched");
        assert_eq!(g.sent(), 62, "only the API's requests count in its total");
    });
}

#[test]
fn cdn_fetches_never_count_toward_the_api_run_total() {
    rt().block_on(async {
        let (g, _) = governor(Limits { per_process: 5, ..Limits::default() }, CdnLimits::default());
        for _ in 0..50 {
            g.cdn(EMOJI, false).unwrap();
        }
        assert_eq!(g.sent(), 0);
        for _ in 0..5 {
            drop(g.admit(Method::ClientCounts, Priority::User).await.expect("the API still goes"));
        }
        assert!(g.is_on());
    });
}

#[test]
fn the_cdn_run_alarm_stops_pictures_and_leaves_the_api_working() {
    rt().block_on(async {
        let cdn = CdnLimits { per_run: 5, ..CdnLimits::default() };
        let (g, clock) = governor(Limits::default(), cdn);
        for _ in 0..5 {
            g.cdn(AVATARS, false).unwrap();
        }
        assert!(matches!(g.cdn(EMOJI, false), Err(SlackError::CapReached { why: CDN_RUN_REACHED })));
        clock.advance(HOUR * 2);
        assert!(matches!(g.cdn(AVATARS, true), Err(SlackError::CapReached { why: CDN_RUN_REACHED })), "the run");
        assert!(g.is_on(), "the network stays on");
        let lane = g.lane();
        g.dial(&lane).expect("the socket still opens");
        drop(g.admit_in(&lane, Method::ClientCounts, Priority::Background).await.expect("a lookup still goes"));
        drop(g.admit(Method::ClientCounts, Priority::User).await.expect("the API still goes"));
        g.picture(&lane).expect("previews keep to the API's rails");
        assert!(g.switch(true), "nothing tripped");
    });
}

#[test]
fn the_cdn_run_alarm_is_told_once() {
    let (g, _) = governor(Limits::default(), CdnLimits { per_run: 1, ..CdnLimits::default() });
    assert!(!g.cdn_alarm_first(), "not before it went off");
    g.cdn(AVATARS, false).unwrap();
    assert!(cap(g.cdn(AVATARS, false)));
    assert!(g.cdn_alarm_first());
    assert!(cap(g.cdn(EMOJI, false)));
    assert!(!g.cdn_alarm_first(), "once");
}

#[test]
fn a_paused_host_refuses_only_itself() {
    let (g, clock) = governor(Limits::default(), CdnLimits::default());
    g.pause_host(EMOJI, Some(Duration::from_secs(5)));
    assert_eq!(g.host_paused(EMOJI), Some(Duration::from_secs(5)));
    assert!(
        matches!(g.cdn(EMOJI, false), Err(SlackError::RateLimited { retry_after: Some(d) }) if d == Duration::from_secs(5))
    );
    g.cdn(AVATARS, false).expect("another host goes on");
    clock.advance(Duration::from_secs(5));
    assert_eq!(g.host_paused(EMOJI), None);
    g.cdn(EMOJI, false).expect("after its pause");
}

#[test]
fn a_host_pause_is_clamped_and_only_lengthens() {
    let (g, _) = governor(Limits::default(), CdnLimits::default());
    g.pause_host(AVATARS, None);
    assert_eq!(g.host_paused(AVATARS), Some(HOST_PAUSE), "none said: the default");
    g.pause_host(AVATARS, Some(Duration::from_secs(2)));
    assert_eq!(g.host_paused(AVATARS), Some(HOST_PAUSE), "never shorter");
    g.pause_host(AVATARS, Some(Duration::from_secs(86_400)));
    assert_eq!(g.host_paused(AVATARS), Some(HOST_PAUSE_MAX), "at most an hour");
    g.pause_host(EMOJI, Some(Duration::ZERO));
    assert_eq!(g.host_paused(EMOJI), Some(Duration::from_secs(1)), "at least a second");
}

#[test]
fn at_most_twelve_cdn_slots_and_switching_off_ends_the_wait() {
    rt().block_on(async {
        let (g, _) = governor(Limits::default(), CdnLimits::default());
        let mut held = Vec::new();
        for _ in 0..12 {
            held.push(g.cdn_slot().await.unwrap());
        }
        let thirteenth = tokio::time::timeout(Duration::from_millis(50), g.cdn_slot()).await;
        assert!(thirteenth.is_err(), "the 13th waits");
        drop(held.pop());
        let freed = tokio::time::timeout(Duration::from_millis(50), g.cdn_slot()).await;
        held.push(freed.expect("a place given back is taken").unwrap());
        let g2 = Arc::clone(&g);
        let waiting = tokio::spawn(async move { g2.cdn_slot().await.map(|_| ()) });
        tokio::task::yield_now().await;
        g.stop();
        let ended = tokio::time::timeout(Duration::from_secs(1), waiting).await.expect("the wait ends").unwrap();
        assert!(matches!(ended, Err(SlackError::Offline { .. })));
        assert!(matches!(g.cdn(AVATARS, false), Err(SlackError::Offline { .. })), "nothing asked when off");
    });
}
