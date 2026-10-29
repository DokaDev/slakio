use super::*;

#[test]
fn the_pause_doubles_within_its_bounds_with_a_random_part() {
    let r = Realtime::default();
    assert_eq!(r.retry_in(0, 0.0), Duration::from_millis(500));
    assert_eq!(r.retry_in(0, 1.0), Duration::from_secs(1));
    assert_eq!(r.retry_in(1, 1.0), Duration::from_secs(2));
    assert_eq!(r.retry_in(4, 1.0), Duration::from_secs(16));
    for failures in [5, 6, 20, u32::MAX] {
        assert_eq!(r.retry_in(failures, 1.0), Duration::from_secs(30), "{failures}");
        assert_eq!(r.retry_in(failures, 0.0), Duration::from_secs(15), "{failures}");
    }
    for failures in 0..40 {
        for jitter in [-1.0, 0.0, 0.3, 0.999, 2.0] {
            let d = r.retry_in(failures, jitter);
            assert!(d >= Duration::from_millis(500) && d <= Duration::from_secs(30), "{failures} {jitter}: {d:?}");
        }
    }
}

#[test]
fn a_wall_clock_far_ahead_of_the_monotonic_one_is_a_sleep_and_nothing_else_is() {
    let r = Realtime::default();
    let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_900_000_000);
    let five = Duration::from_secs(5);
    assert!(!r.slept(five, t0, t0 + five), "both moved on alike");
    assert!(!r.slept(five, t0, t0 + five + r.wake_gap), "within the gap");
    assert!(r.slept(five, t0, t0 + five + r.wake_gap + Duration::from_millis(1)));
    assert!(r.slept(five, t0, t0 + Duration::from_secs(3600)), "an hour with the lid closed");
    assert!(!r.slept(five, t0, t0 - Duration::from_secs(3600)), "a clock set back is no sleep");
    assert!(!r.slept(Duration::from_secs(3600), t0, t0 + Duration::from_secs(3600)), "a long check, awake");
}
