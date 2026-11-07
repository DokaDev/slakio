use super::*;
use proptest::prelude::*;

fn utc() -> Zone {
    Zone::utc()
}

fn kst() -> Zone {
    Zone::fixed(9 * 3600).unwrap()
}

fn seoul() -> Zone {
    Zone::named("Asia/Seoul").expect("the system knows Asia/Seoul")
}

fn new_york() -> Zone {
    Zone::named("America/New_York").expect("the system knows America/New_York")
}

/// A Slack time of `y-m-d h:m` UTC.
fn at(y: i64, m: u32, d: u32, h: u64, min: u64) -> Ts {
    let day = u64::try_from(days(y, m, d).unwrap()).unwrap();
    Ts(((day * 86_400) + h * 3600 + min * 60) * 1_000_000)
}

#[test]
fn dates_and_times_of_slack_timestamps() {
    let ts = Ts(1_767_603_600_000_100);
    assert_eq!((date(ts, &utc()), hm(ts, &utc())), ("2026-01-05".to_string(), "09:00".to_string()));
    assert_eq!(date(Ts(0), &utc()), "1970-01-01");
    assert_eq!(date(Ts(951_782_400 * 1_000_000), &utc()), "2000-02-29", "a leap day");
    assert_eq!(hm(Ts((86_400 - 60) * 1_000_000), &utc()), "23:59");
    assert_eq!(day(Ts(86_399_999_999), &utc()), 0);
    assert_eq!(day(Ts(86_400_000_000), &utc()), 1);
}

#[test]
fn a_civil_date_is_its_days_and_back() {
    let d = |ts| day(ts, &utc());
    assert_eq!(days(1970, 1, 1), Some(0));
    assert_eq!(days(2026, 1, 5), Some(d(Ts(1_767_603_600_000_100))));
    assert_eq!(days(2000, 2, 29).map(ymd), Some((2000, 2, 29)));
    assert_eq!(days(2026, 2, 29), None, "not a leap year");
    assert_eq!(days(2026, 4, 31), None);
    assert_eq!(days(2026, 13, 1), None);
    assert_eq!(days(2026, 1, 0), None);
    assert_eq!(days(1969, 12, 31), None, "before Slack's clock");
    for d in [0, 1, 59, 365, 10_957, 20_000, 2_932_896] {
        let (y, m, dd) = ymd(d);
        assert_eq!(days(y, m, dd), Some(d), "{d}");
    }
}

#[test]
fn korea_s_times_and_days_are_nine_hours_ahead() {
    // 23:30 UTC on the 6th is 08:30 on the 7th in Seoul.
    let ts = at(2026, 11, 6, 23, 30);
    for z in [kst(), seoul()] {
        assert_eq!((date(ts, &z), hm(ts, &z)), ("2026-11-07".to_string(), "08:30".to_string()));
        assert_eq!(day(ts, &z), days(2026, 11, 7).unwrap());
        // The 7th starts at 15:00 UTC on the 6th.
        assert_eq!(z.start(days(2026, 11, 7).unwrap()), at(2026, 11, 6, 15, 0));
        // A weekday changes at local midnight: 14:59 UTC is still the 6th (a Friday), 15:00 the
        // 7th (a Saturday).
        assert_eq!(day(at(2026, 11, 6, 14, 59), &z), days(2026, 11, 6).unwrap());
        assert_eq!(day(at(2026, 11, 6, 15, 0), &z), days(2026, 11, 7).unwrap());
    }
    assert_eq!(date(ts, &utc()), "2026-11-06", "UTC still on the 6th");
}

#[test]
fn a_day_the_clocks_change_is_23_or_25_hours_long_and_starts_at_local_midnight() {
    let z = new_york();
    // 2026-03-08: clocks go from 02:00 to 03:00 (EST, UTC-5, to EDT, UTC-4).
    let spring = days(2026, 3, 8).unwrap();
    assert_eq!(z.start(spring), at(2026, 3, 8, 5, 0));
    assert_eq!(z.start(spring + 1), at(2026, 3, 9, 4, 0));
    assert_eq!(z.start(spring + 1).0 - z.start(spring).0, 23 * 3_600_000_000);
    // 2026-11-01: clocks go from 02:00 back to 01:00.
    let fall = days(2026, 11, 1).unwrap();
    assert_eq!(z.start(fall), at(2026, 11, 1, 4, 0));
    assert_eq!(z.start(fall + 1), at(2026, 11, 2, 5, 0));
    assert_eq!(z.start(fall + 1).0 - z.start(fall).0, 25 * 3_600_000_000);
    // The hour shown twice: 01:30 EDT, then 01:30 EST.
    assert_eq!(hm(at(2026, 11, 1, 5, 30), &z), "01:30");
    assert_eq!(hm(at(2026, 11, 1, 6, 30), &z), "01:30");
    assert_eq!(day(at(2026, 11, 2, 4, 59), &z), fall, "23:59 EST is still the 1st");
}

#[test]
fn a_zone_is_named_by_the_environment_first_then_tz() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |k: &str| pairs.iter().find(|(key, _)| *key == k).map(|(_, v)| (*v).to_string())
    };
    let ts = at(2026, 11, 6, 23, 30);
    let hm_of = |z: Zone| hm(ts, &z);
    assert_eq!(hm_of(Zone::from_env(env(&[(ZONE_ENV, "Asia/Seoul"), ("TZ", "UTC")]))), "08:30");
    assert_eq!(hm_of(Zone::from_env(env(&[(ZONE_ENV, "UTC"), ("TZ", "Asia/Seoul")]))), "23:30");
    assert_eq!(hm_of(Zone::from_env(env(&[("TZ", ":Asia/Seoul")]))), "08:30");
    assert_eq!(hm_of(Zone::from_env(env(&[(ZONE_ENV, "KST-9")]))), "08:30", "a POSIX rule");
    // A name not understood is passed over.
    assert_eq!(hm_of(Zone::from_env(env(&[(ZONE_ENV, "Mars/Olympus"), ("TZ", "Asia/Seoul")]))), "08:30");
    assert_eq!(Zone::named(""), None);
    assert_eq!(Zone::named("Not/AZone"), None);
}

proptest! {
    /// In any zone a fixed offset from UTC (up to ±14 h), a day's start is in that day, and any
    /// time is in the day it names, between its start and the next day's.
    #[test]
    fn a_day_and_its_start_round_trip(secs in -14i32 * 3600..=14 * 3600, ts in 0u64..4_102_444_800_000_000) {
        let z = Zone::fixed(secs).unwrap();
        let ts = Ts(ts);
        let d = day(ts, &z);
        if d >= 1 {
            prop_assert_eq!(day(z.start(d), &z), d);
            prop_assert!(z.start(d) <= ts && ts < z.start(d + 1), "{ts:?} in day {d}");
        }
        prop_assert_eq!(date(ts, &z), {
            let (y, m, dd) = ymd(d);
            format!("{y:04}-{m:02}-{dd:02}")
        });
    }
}
