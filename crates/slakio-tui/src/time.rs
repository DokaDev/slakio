//! Message times on screen: the day (`2026-01-05`) of the date separators and the time of day
//! (`10:02`) beside each message, from a Slack timestamp. Shown in UTC for now: the demo's
//! times are invented, and the user's time zone comes with real workspaces.

use slakio_core::model::Ts;

/// The civil date (year, month, day) of `days` since 1970-01-01 (proleptic Gregorian).
fn civil(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's days-from-civil, inverted.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// The day of `ts`, for comparing and for the date separator.
pub fn day(ts: Ts) -> i64 {
    (ts.secs() / 86_400) as i64
}

/// `2026-01-05`.
pub fn date(ts: Ts) -> String {
    let (y, m, d) = civil(day(ts));
    format!("{y:04}-{m:02}-{d:02}")
}

/// `10:02`.
pub fn hm(ts: Ts) -> String {
    let s = ts.secs() % 86_400;
    format!("{:02}:{:02}", s / 3600, s % 3600 / 60)
}

#[cfg(test)]
mod tests;
