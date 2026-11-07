//! The day `:jump <date>` names, in the days the screen shows (the user's time zone, as the
//! date separators): `today`, `yesterday`, `2026-11-01` (also `/` or `.` between), `11-01`
//! (month and day: this year's, or the last one already past; `02-29` the last leap year's),
//! `-3d` / `3d` (days ago), `-2w` / `2w` (weeks ago), a weekday (`mon`, `monday`: the last one,
//! today included), and the same words in the language shown ([`Words`]). Case and spaces
//! around are ignored; anything else is no day.

use crate::time::{Zone, days, ymd};
use slakio_core::model::Ts;

/// How far back a count of days or weeks may reach.
const BACK_MAX: i64 = 36_500;

const WEEKDAYS: [&str; 7] = ["thursday", "friday", "saturday", "sunday", "monday", "tuesday", "wednesday"];

/// How many years back `MM-DD` looks for one that has that day (`02-29`).
const LEAP_BACK: i64 = 8;

/// The words for days of the language shown, beside the English ones (which always work):
/// today, yesterday, the day before yesterday, and the weekdays from Sunday. A weekday is also
/// named by its first character when that is not ASCII (the first character of a Korean weekday).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Words {
    pub today: String,
    pub yesterday: String,
    pub before_yesterday: String,
    pub weekdays: Vec<String>,
}

impl Words {
    /// The day `t` (lower case, trimmed) names by these words.
    fn day(&self, t: &str, today: i64) -> Option<i64> {
        let is = |w: &str| !w.is_empty() && w.to_lowercase() == t;
        if is(&self.today) {
            return Some(today);
        }
        if is(&self.yesterday) {
            return Some(today - 1);
        }
        if is(&self.before_yesterday) {
            return Some(today - 2);
        }
        let first = t.chars().next().filter(|c| !c.is_ascii() && t.chars().count() == 1);
        // From Sunday: 1970-01-04 was one.
        let sunday = self.weekdays.iter().position(|w| is(w) || first.is_some_and(|c| w.starts_with(c)))?;
        let w = (sunday as i64 + 3) % 7;
        Some(today - (today - w).rem_euclid(7))
    }
}

/// The day `text` names, `today` being the day it is (days since 1970-01-01); `None` when it
/// names none. A day still to come is named too (the caller refuses it).
pub fn parse(text: &str, today: i64, words: &Words) -> Option<i64> {
    let t = text.trim().to_lowercase();
    match t.as_str() {
        "today" | "now" => return Some(today),
        "yesterday" => return Some(today - 1),
        _ => {}
    }
    if let Some(d) = words.day(&t, today) {
        return Some(d).filter(|d| *d >= 0);
    }
    if let Some(n) = ago(&t) {
        return Some(today - n).filter(|d| *d >= 0);
    }
    if t.len() >= 3
        && let Some(w) = WEEKDAYS.iter().position(|w| w.starts_with(t.as_str()))
    {
        // 1970-01-01 was a Thursday (the first of the list).
        let back = (today - w as i64).rem_euclid(7);
        return Some(today - back).filter(|d| *d >= 0);
    }
    let parts: Vec<&str> = t.split(['-', '/', '.']).collect();
    let num = |s: &str| {
        (!s.is_empty() && s.len() <= 4 && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
    };
    match parts[..] {
        [y, m, d] if y.len() == 4 => days(num(y)?, u32::try_from(num(m)?).ok()?, u32::try_from(num(d)?).ok()?),
        [m, d] => {
            let (m, d) = (u32::try_from(num(m)?).ok()?, u32::try_from(num(d)?).ok()?);
            // The last such day not to come yet: this year's, else an earlier year's (a leap
            // day's is up to 8 years back).
            let (year, ..) = ymd(today);
            (0..=LEAP_BACK).filter_map(|back| days(year - back, m, d)).find(|day| *day <= today)
        }
        _ => None,
    }
}

/// The days `-3d` / `3d` / `-2w` / `2w` go back.
fn ago(t: &str) -> Option<i64> {
    let t = t.strip_prefix('-').unwrap_or(t);
    let (n, per) = match t.strip_suffix('d') {
        Some(n) => (n, 1),
        None => (t.strip_suffix('w')?, 7),
    };
    if n.is_empty() || n.len() > 6 || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: i64 = n.parse().ok()?;
    Some(n * per).filter(|d| *d <= BACK_MAX)
}

/// The start of `day` in `zone` as a Slack time.
pub fn start(day: i64, zone: &Zone) -> Ts {
    zone.start(day)
}

#[cfg(test)]
mod tests;
