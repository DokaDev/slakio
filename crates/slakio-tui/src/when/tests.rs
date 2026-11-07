use super::*;
use proptest::prelude::*;

/// 2026-11-07, a Saturday.
fn today() -> i64 {
    days(2026, 11, 7).unwrap()
}

fn day(y: i64, m: u32, d: u32) -> Option<i64> {
    days(y, m, d)
}

#[test]
fn words_dates_and_counts_name_their_day() {
    let t = today();
    assert_eq!(parse("today", t), Some(t));
    assert_eq!(parse(" Yesterday ", t), Some(t - 1));
    assert_eq!(parse("2026-11-01", t), day(2026, 11, 1));
    assert_eq!(parse("2026/11/01", t), day(2026, 11, 1));
    assert_eq!(parse("2026.1.5", t), day(2026, 1, 5));
    assert_eq!(parse("11-01", t), day(2026, 11, 1));
    assert_eq!(parse("12-25", t), day(2025, 12, 25), "still to come: last year's");
    assert_eq!(parse("11-07", t), Some(t));
    assert_eq!(parse("-3d", t), Some(t - 3));
    assert_eq!(parse("3d", t), Some(t - 3));
    assert_eq!(parse("-2w", t), Some(t - 14));
    assert_eq!(parse("0d", t), Some(t));
    assert_eq!(parse("sat", t), Some(t), "today is a Saturday");
    assert_eq!(parse("Monday", t), day(2026, 11, 2));
    assert_eq!(parse("fri", t), day(2026, 11, 6));
    assert_eq!(parse("sunday", t), day(2026, 11, 1));
}

#[test]
fn anything_else_is_no_day() {
    let t = today();
    for bad in [
        "",
        "tomorrow",
        "2026-02-30",
        "2026-13-01",
        "26-11-01",
        "11-32",
        "-d",
        "d",
        "3x",
        "--3d",
        "3dd",
        "1000000d",
        "99999w",
        "mo",
        "2026-11",
        "2026-11-01-02",
        "+3d",
        "1e3d",
        "\u{1b}[2J",
        "2026\u{ff0d}11\u{ff0d}01",
    ] {
        assert_eq!(parse(bad, t), None, "{bad:?}");
    }
    assert_eq!(parse("-40000d", t), None, "too far back");
    assert_eq!(parse("1969-12-31", t), None);
}

#[test]
fn a_day_starts_at_midnight() {
    assert_eq!(start(1), Ts(86_400_000_000));
    assert_eq!(start(-5), Ts(0));
}

proptest! {
    /// Any text: no panic, and a day named is never after today nor before 1970.
    #[test]
    fn any_text_names_a_past_day_or_none(text in any::<String>(), back in 0i64..20_000) {
        let t = today();
        if let Some(d) = parse(&text, t) {
            prop_assert!(d >= 0);
            let dated = text.trim().split(['-', '/', '.']).count() == 3;
            prop_assert!(d <= t || dated, "{text:?} -> {d}");
        }
        prop_assert_eq!(parse(&format!("-{back}d"), t), Some(t - back).filter(|d| *d >= 0));
    }

    /// Every date written out reads back as itself.
    #[test]
    fn a_date_written_out_reads_back(d in 0i64..2_900_000) {
        let (y, m, dd) = ymd(d);
        prop_assert_eq!(parse(&format!("{y:04}-{m:02}-{dd:02}"), today()), Some(d));
        prop_assert_eq!(parse(&format!("{y}/{m}/{dd}"), today()), Some(d));
    }
}
