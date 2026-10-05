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
