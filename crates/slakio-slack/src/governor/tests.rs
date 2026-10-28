use super::*;

const M: Method = Method::ClientCounts;

fn governor(limits: Limits, on: bool) -> (Arc<Governor>, FakeClock) {
    let clock = FakeClock::default();
    (Governor::new(limits, Box::new(clock.clone()), on), clock)
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

#[test]
fn the_switch_reads_the_environment_and_the_config() {
    assert_eq!(network_on(None, "on"), Ok(true));
    assert_eq!(network_on(None, "off"), Ok(false));
    assert_eq!(network_on(Some("OFF"), "on"), Ok(false));
    assert_eq!(network_on(Some("on"), "off"), Ok(false), "either off wins");
    assert_eq!(network_on(Some(""), "on"), Ok(true));
    assert!(network_on(Some("0"), "on").is_err(), "a typo never leaves the network on");
}

#[test]
fn with_the_network_off_nothing_is_let_through() {
    rt().block_on(async {
        let (g, _) = governor(Limits::default(), false);
        assert!(matches!(g.admit(M, Priority::User).await, Err(SlackError::Offline { .. })));
        let (g, _) = governor(Limits::default(), true);
        drop(g.admit(M, Priority::User).await.unwrap());
        g.stop();
        assert!(matches!(g.admit(M, Priority::User).await, Err(SlackError::Offline { .. })));
        assert_eq!(g.sent(), 1);
        tokio::time::timeout(Duration::from_secs(1), g.stopped()).await.expect("stopped is done once off");
    });
}

#[test]
fn the_minute_cap_keeps_room_for_the_user_and_frees_up_with_time() {
    rt().block_on(async {
        let limits = Limits { per_minute: 10, background_per_minute: 6, ..Limits::default() };
        let (g, clock) = governor(limits, true);
        for _ in 0..6 {
            drop(g.admit(M, Priority::Background).await.unwrap());
        }
        assert!(matches!(g.admit(M, Priority::Background).await, Err(SlackError::CapReached { .. })));
        for _ in 0..4 {
            drop(g.admit(M, Priority::User).await.unwrap());
        }
        assert!(matches!(g.admit(M, Priority::User).await, Err(SlackError::CapReached { .. })));
        clock.advance(Duration::from_secs(59));
        assert!(g.admit(M, Priority::User).await.is_err(), "the window is a full minute");
        clock.advance(Duration::from_secs(1));
        drop(g.admit(M, Priority::Background).await.unwrap());
        assert!(g.is_on(), "the minute cap does not switch the network off");
    });
}

#[test]
fn the_cap_of_a_run_stops_all_network_until_restart() {
    rt().block_on(async {
        let (g, clock) = governor(Limits { per_process: 3, ..Limits::default() }, true);
        assert_eq!(Limits::default().per_process, 10_000);
        for _ in 0..3 {
            drop(g.admit(M, Priority::User).await.unwrap());
        }
        assert!(matches!(g.admit(M, Priority::User).await, Err(SlackError::CapReached { .. })));
        clock.advance(Duration::from_secs(3600));
        assert!(g.admit(M, Priority::User).await.is_err());
        assert!(!g.is_on());
        assert_eq!(g.sent(), 3);
    });
}

#[test]
fn at_most_eight_in_flight_and_more_waiters_are_busy() {
    rt().block_on(async {
        let (g, _) = governor(Limits { waiting: 1, ..Limits::default() }, true);
        let mut held = Vec::new();
        for _ in 0..8 {
            held.push(g.admit(M, Priority::User).await.unwrap());
        }
        let waiter = {
            let g = Arc::clone(&g);
            tokio::spawn(async move { g.admit(M, Priority::User).await.map(drop) })
        };
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished(), "the ninth waits for a slot");
        assert_eq!(g.admit(M, Priority::User).await.unwrap_err(), SlackError::Busy, "the queue is full");
        held.pop();
        waiter.await.unwrap().unwrap();
        assert_eq!(g.sent(), 9);
    });
}

#[test]
fn switching_off_ends_the_wait_for_a_slot() {
    rt().block_on(async {
        let (g, _) = governor(Limits { in_flight: 1, ..Limits::default() }, true);
        let _held = g.admit(M, Priority::User).await.unwrap();
        let waiter = {
            let g = Arc::clone(&g);
            tokio::spawn(async move { g.admit(M, Priority::User).await.map(drop) })
        };
        tokio::task::yield_now().await;
        g.stop();
        assert!(matches!(waiter.await.unwrap(), Err(SlackError::Offline { .. })));
        assert_eq!(g.sent(), 1);
    });
}

#[test]
fn the_hourly_budget_refuses_without_stopping_and_frees_up_with_the_hour() {
    rt().block_on(async {
        let (g, clock) = governor(Limits { per_hour: 5, ..Limits::default() }, true);
        assert_eq!(Limits::default().per_hour, 1500);
        for _ in 0..5 {
            drop(g.admit(M, Priority::User).await.unwrap());
            clock.advance(Duration::from_secs(61));
        }
        let e = g.admit(M, Priority::User).await.unwrap_err();
        assert_eq!(e, SlackError::CapReached { why: "the hourly request budget is reached" });
        assert!(g.is_on(), "a budget is not a stop");
        // The first one leaves the hour.
        clock.advance(Duration::from_secs(3600 - 5 * 61 + 1));
        drop(g.admit(M, Priority::User).await.unwrap());
    });
}

#[test]
fn a_rate_limited_method_waits_its_retry_after_and_the_others_go_on() {
    rt().block_on(async {
        let (g, clock) = governor(Limits::default(), true);
        g.pause(M, Some(Duration::from_secs(7)));
        assert_eq!(
            g.admit(M, Priority::User).await.unwrap_err(),
            SlackError::RateLimited { retry_after: Some(Duration::from_secs(7)) }
        );
        drop(g.admit(Method::ConversationsView, Priority::User).await.unwrap());
        clock.advance(Duration::from_secs(7));
        drop(g.admit(M, Priority::User).await.unwrap());
        // Without a Retry-After, the default pause.
        g.pause(M, None);
        clock.advance(DEFAULT_PAUSE - Duration::from_secs(1));
        assert!(g.admit(M, Priority::User).await.is_err());
        clock.advance(Duration::from_secs(1));
        drop(g.admit(M, Priority::User).await.unwrap());
        assert_eq!(g.sent(), 3, "a paused method is never counted");
    });
}

#[test]
fn a_busy_workspace_never_takes_every_slot_from_another() {
    rt().block_on(async {
        let limits = Limits { in_flight: 4, lane_in_flight: 3, lane_waiting: 1, ..Limits::default() };
        let (g, _) = governor(limits, true);
        let (a, b) = (g.lane(), g.lane());
        let mut held = Vec::new();
        for _ in 0..3 {
            held.push(g.admit_in(&a, M, Priority::User).await.unwrap());
        }
        // A's fourth waits for one of A's own slots, though the process has one free.
        let waiting = g.admit_in(&a, M, Priority::User);
        tokio::pin!(waiting);
        assert!(tokio::time::timeout(Duration::from_millis(50), &mut waiting).await.is_err(), "A waits");
        // Meanwhile B gets the free slot at once.
        let b1 = tokio::time::timeout(Duration::from_millis(50), g.admit_in(&b, M, Priority::User)).await;
        let b1 = b1.expect("B is not held back by A").unwrap();
        // A's waiting places are taken by the one waiting: another of A's is refused as busy.
        assert!(matches!(g.admit_in(&a, M, Priority::User).await, Err(SlackError::Busy)));
        drop(held.pop());
        let a4 = tokio::time::timeout(Duration::from_secs(1), &mut waiting).await.expect("A's turn").unwrap();
        assert_eq!(g.sent(), 5);
        drop((a4, b1, held));
        // Switched off: a lane's waiting ends too.
        let (g, _) = governor(Limits { in_flight: 1, lane_in_flight: 1, ..Limits::default() }, true);
        let lane = g.lane();
        let first = g.admit_in(&lane, M, Priority::User).await.unwrap();
        let second = g.admit_in(&lane, M, Priority::User);
        tokio::pin!(second);
        assert!(tokio::time::timeout(Duration::from_millis(20), &mut second).await.is_err());
        g.stop();
        assert!(matches!(second.await, Err(SlackError::Offline { .. })));
        drop(first);
    });
}

#[test]
fn a_waiting_request_given_up_frees_its_waiting_place() {
    rt().block_on(async {
        // In a lane: the one waiting place is given back when the waiting request is dropped.
        let (g, _) = governor(Limits { in_flight: 1, lane_in_flight: 1, lane_waiting: 1, ..Limits::default() }, true);
        let lane = g.lane();
        let first = g.admit_in(&lane, M, Priority::User).await.unwrap();
        for _ in 0..3 {
            let waiting = g.admit_in(&lane, M, Priority::User);
            tokio::pin!(waiting);
            assert!(tokio::time::timeout(Duration::from_millis(20), &mut waiting).await.is_err(), "it waits");
        }
        let next = g.admit_in(&lane, M, Priority::User);
        tokio::pin!(next);
        assert!(tokio::time::timeout(Duration::from_millis(20), &mut next).await.is_err(), "it waits, not busy");
        drop(first);
        drop(tokio::time::timeout(Duration::from_secs(1), next).await.expect("its turn").unwrap());
        // The process's waiting places, the same.
        let (g, _) = governor(Limits { in_flight: 1, waiting: 1, ..Limits::default() }, true);
        let first = g.admit(M, Priority::User).await.unwrap();
        for _ in 0..3 {
            let waiting = g.admit(M, Priority::User);
            tokio::pin!(waiting);
            assert!(tokio::time::timeout(Duration::from_millis(20), &mut waiting).await.is_err(), "it waits");
        }
        let next = g.admit(M, Priority::User);
        tokio::pin!(next);
        assert!(tokio::time::timeout(Duration::from_millis(20), &mut next).await.is_err(), "it waits, not busy");
        drop(first);
        drop(tokio::time::timeout(Duration::from_secs(1), next).await.expect("its turn").unwrap());
    });
}

#[test]
fn busy_workspaces_never_take_every_slot_from_a_third() {
    rt().block_on(async {
        let (g, _) = governor(Limits::default(), true);
        let (a, b, c) = (g.lane(), g.lane(), g.lane());
        let wait = Duration::from_millis(20);
        let mut held = Vec::new();
        for _ in 0..6 {
            held.push(tokio::time::timeout(wait, g.admit_in(&a, M, Priority::User)).await.expect("A's share").unwrap());
        }
        held.push(tokio::time::timeout(wait, g.admit_in(&b, M, Priority::User)).await.expect("B's first").unwrap());
        let b2 = g.admit_in(&b, M, Priority::User);
        tokio::pin!(b2);
        let b2_now = tokio::time::timeout(wait, &mut b2).await;
        // A and B busy: C still gets a slot at once.
        let c1 = tokio::time::timeout(wait, g.admit_in(&c, M, Priority::User)).await;
        let c1 = c1.expect("C is not held back by A and B").unwrap();
        assert!(b2_now.is_err(), "B's second waits for a slot, C's is kept");
        drop((c1, held));
        drop(tokio::time::timeout(Duration::from_secs(1), b2).await.expect("B's turn").unwrap());
    });
}
