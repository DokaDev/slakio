use super::*;

#[test]
fn dates_and_times_of_slack_timestamps() {
    let ts = Ts(1_767_603_600_000_100);
    assert_eq!((date(ts), hm(ts)), ("2026-01-05".to_string(), "09:00".to_string()));
    assert_eq!(date(Ts(0)), "1970-01-01");
    assert_eq!(date(Ts(951_782_400 * 1_000_000)), "2000-02-29", "a leap day");
    assert_eq!(hm(Ts((86_400 - 60) * 1_000_000)), "23:59");
    assert_eq!(day(Ts(86_399_999_999)), 0);
    assert_eq!(day(Ts(86_400_000_000)), 1);
}

#[test]
fn a_civil_date_is_its_days_and_back() {
    assert_eq!(days(1970, 1, 1), Some(0));
    assert_eq!(days(2026, 1, 5), Some(day(Ts(1_767_603_600_000_100))));
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
