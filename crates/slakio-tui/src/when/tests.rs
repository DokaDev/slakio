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
    assert_eq!(parse("today", t, &en()), Some(t));
    assert_eq!(parse(" Yesterday ", t, &en()), Some(t - 1));
    assert_eq!(parse("2026-11-01", t, &en()), day(2026, 11, 1));
    assert_eq!(parse("2026/11/01", t, &en()), day(2026, 11, 1));
    assert_eq!(parse("2026.1.5", t, &en()), day(2026, 1, 5));
    assert_eq!(parse("11-01", t, &en()), day(2026, 11, 1));
    assert_eq!(parse("12-25", t, &en()), day(2025, 12, 25), "still to come: last year's");
    assert_eq!(parse("11-07", t, &en()), Some(t));
    assert_eq!(parse("-3d", t, &en()), Some(t - 3));
    assert_eq!(parse("3d", t, &en()), Some(t - 3));
    assert_eq!(parse("-2w", t, &en()), Some(t - 14));
    assert_eq!(parse("0d", t, &en()), Some(t));
    assert_eq!(parse("sat", t, &en()), Some(t), "today is a Saturday");
    assert_eq!(parse("Monday", t, &en()), day(2026, 11, 2));
    assert_eq!(parse("fri", t, &en()), day(2026, 11, 6));
    assert_eq!(parse("sunday", t, &en()), day(2026, 11, 1));
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
        assert_eq!(parse(bad, t, &en()), None, "{bad:?}");
    }
    assert_eq!(parse("-40000d", t, &en()), None, "too far back");
    assert_eq!(parse("1969-12-31", t, &en()), None);
}

#[test]
fn a_day_starts_at_midnight() {
    assert_eq!(start(1, &Zone::utc()), Ts(86_400_000_000));
    assert_eq!(start(-5, &Zone::utc()), Ts(0));
    // In Seoul, nine hours earlier.
    assert_eq!(start(1, &Zone::fixed(9 * 3600).unwrap()), Ts(86_400_000_000 - 9 * 3_600_000_000));
}

/// No words of another language.
fn en() -> Words {
    Words::default()
}

/// Korean's words for days (written with escapes: the code stays ASCII).
fn ko() -> Words {
    let w = |s: &str| s.to_string();
    Words {
        today: w("\u{C624}\u{B298}"),
        yesterday: w("\u{C5B4}\u{C81C}"),
        before_yesterday: w("\u{ADF8}\u{C81C}"),
        weekdays: ["\u{C77C}", "\u{C6D4}", "\u{D654}", "\u{C218}", "\u{BAA9}", "\u{AE08}", "\u{D1A0}"]
            .map(|d| format!("{d}\u{C694}\u{C77C}"))
            .to_vec(),
    }
}

#[test]
fn a_leap_day_without_one_this_year_is_the_last_leap_year_s() {
    let t = today();
    assert_eq!(parse("02-29", t, &en()), day(2024, 2, 29));
    assert_eq!(parse("2-29", t, &en()), day(2024, 2, 29));
    assert_eq!(parse("02-30", t, &en()), None);
    // In 2028, its own, once it has come.
    let late_2028 = days(2028, 3, 1).unwrap();
    assert_eq!(parse("02-29", late_2028, &en()), day(2028, 2, 29));
    let early_2028 = days(2028, 2, 1).unwrap();
    assert_eq!(parse("02-29", early_2028, &en()), day(2024, 2, 29));
}

#[test]
fn the_language_s_own_words_name_days_too() {
    let t = today();
    let k = ko();
    assert_eq!(parse("\u{C624}\u{B298}", t, &k), Some(t));
    assert_eq!(parse(" \u{C5B4}\u{C81C} ", t, &k), Some(t - 1));
    assert_eq!(parse("\u{ADF8}\u{C81C}", t, &k), Some(t - 2));
    // Monday, by its whole word or its first character; today a Saturday.
    assert_eq!(parse("\u{C6D4}\u{C694}\u{C77C}", t, &k), day(2026, 11, 2));
    assert_eq!(parse("\u{C6D4}", t, &k), day(2026, 11, 2));
    assert_eq!(parse("\u{D1A0}\u{C694}\u{C77C}", t, &k), Some(t), "Saturday: today");
    assert_eq!(parse("\u{C77C}", t, &k), day(2026, 11, 1), "Sunday");
    // English still works; the words of another language do not without them.
    assert_eq!(parse("monday", t, &k), day(2026, 11, 2));
    assert_eq!(parse("\u{C624}\u{B298}", t, &en()), None);
    assert_eq!(parse("\u{C6D4}\u{C694}", t, &k), None, "not a word of the list");
}

proptest! {
    /// Any text: no panic, and a day named is never after today nor before 1970.
    #[test]
    fn any_text_names_a_past_day_or_none(text in any::<String>(), back in 0i64..20_000) {
        let t = today();
        if let Some(d) = parse(&text, t, &en()) {
            prop_assert!(d >= 0);
            let dated = text.trim().split(['-', '/', '.']).count() == 3;
            prop_assert!(d <= t || dated, "{text:?} -> {d}");
        }
        prop_assert_eq!(parse(&format!("-{back}d"), t, &en()), Some(t - back).filter(|d| *d >= 0));
    }

    /// Every date written out reads back as itself.
    #[test]
    fn a_date_written_out_reads_back(d in 0i64..2_900_000) {
        let (y, m, dd) = ymd(d);
        prop_assert_eq!(parse(&format!("{y:04}-{m:02}-{dd:02}"), today(), &en()), Some(d));
        prop_assert_eq!(parse(&format!("{y}/{m}/{dd}"), today(), &en()), Some(d));
    }
}
