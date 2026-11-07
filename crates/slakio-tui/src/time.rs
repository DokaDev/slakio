//! Message times on screen: the day (`2026-01-05`) of the date separators and the time of day
//! (`10:02`) beside each message, from a Slack timestamp, in the user's time zone ([`Zone`]).
//! A day is a civil day of that zone: days since 1970-01-01 of its calendar, whatever its
//! offset (on a day the clocks change, it is 23 or 25 hours long).

use jiff::Timestamp;
use jiff::tz::{Offset, TimeZone};
use slakio_core::model::Ts;

/// The environment variable that names the time zone the times are shown in (an IANA name such
/// as `Asia/Seoul`, or a POSIX rule such as `KST-9`), before `TZ` and the system's own.
pub const ZONE_ENV: &str = "SLAKIO_TZ";

/// The time zone times and days are shown in; read once at the start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Zone(TimeZone);

impl Default for Zone {
    fn default() -> Self {
        Self::utc()
    }
}

impl Zone {
    pub fn utc() -> Self {
        Self(TimeZone::UTC)
    }

    /// A zone `secs` seconds ahead of UTC all year (`None` past ±25 hours).
    pub fn fixed(secs: i32) -> Option<Self> {
        Offset::from_seconds(secs).ok().map(|o| Self(TimeZone::fixed(o)))
    }

    /// The zone `name` names: an IANA name (`Asia/Seoul`, `UTC`) or a POSIX rule (`KST-9`),
    /// with or without a leading `:`; `None` for anything else.
    pub fn named(name: &str) -> Option<Self> {
        let name = name.trim();
        let name = name.strip_prefix(':').unwrap_or(name);
        if name.is_empty() {
            return None;
        }
        TimeZone::get(name).or_else(|_| TimeZone::posix(name)).ok().map(Self)
    }

    /// The zone to show times in: [`ZONE_ENV`], else `TZ`, else the system's, else UTC (`env`
    /// reads a variable). A name not understood is passed over, never an error.
    pub fn from_env(env: impl Fn(&str) -> Option<String>) -> Self {
        let named = |key: &str| env(key).as_deref().and_then(Self::named);
        named(ZONE_ENV).or_else(|| named("TZ")).or_else(|| TimeZone::try_system().ok().map(Self)).unwrap_or_default()
    }

    /// Seconds the zone is ahead of UTC at `ts`.
    fn offset(&self, ts: Ts) -> i64 {
        let at = Timestamp::from_microsecond(i64::try_from(ts.0).unwrap_or(i64::MAX)).unwrap_or(Timestamp::MAX);
        i64::from(self.0.to_offset(at).seconds())
    }

    /// Seconds since 1970-01-01 00:00 of the zone's calendar at `ts`.
    fn local(&self, ts: Ts) -> i64 {
        i64::try_from(ts.secs()).unwrap_or(i64::MAX).saturating_add(self.offset(ts))
    }

    /// When the zone's civil day `day` starts (its first moment: midnight, or later when the
    /// clocks skip it), as a Slack time; days before 1970 start at 0.
    pub fn start(&self, day: i64) -> Ts {
        let (y, m, d) = civil(day);
        let at = i16::try_from(y)
            .ok()
            .and_then(|y| jiff::civil::Date::new(y, i8::try_from(m).ok()?, i8::try_from(d).ok()?).ok())
            .and_then(|date| date.to_zoned(self.0.clone()).ok())
            .and_then(|z| z.start_of_day().ok())
            .map_or(0, |z| z.timestamp().as_microsecond());
        Ts(u64::try_from(at).unwrap_or(0))
    }
}

/// The time now, by this machine's clock.
pub fn now() -> Ts {
    Ts(u64::try_from(Timestamp::now().as_microsecond()).unwrap_or(0))
}

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

/// The days since 1970-01-01 of the civil date `y-m-d`; `None` when there is no such day.
pub fn days(y: i64, m: u32, d: u32) -> Option<i64> {
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let last = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if d == 0 || d > last || !(1970..=9999).contains(&y) {
        return None;
    }
    // Howard Hinnant's days-from-civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// The civil date of `day` (days since 1970-01-01).
pub fn ymd(day: i64) -> (i64, u32, u32) {
    civil(day)
}

/// The day of `ts` in `zone`, for comparing and for the date separator.
pub fn day(ts: Ts, zone: &Zone) -> i64 {
    zone.local(ts).div_euclid(86_400)
}

/// `2026-01-05`, in `zone`.
pub fn date(ts: Ts, zone: &Zone) -> String {
    let (y, m, d) = civil(day(ts, zone));
    format!("{y:04}-{m:02}-{d:02}")
}

/// `10:02`, in `zone`.
pub fn hm(ts: Ts, zone: &Zone) -> String {
    let s = zone.local(ts).rem_euclid(86_400);
    format!("{:02}:{:02}", s / 3600, s % 3600 / 60)
}

#[cfg(test)]
mod tests;
