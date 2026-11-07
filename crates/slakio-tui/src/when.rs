//! The day `:jump <date>` names, in the days the screen shows (UTC, as the date separators):
//! `today`, `yesterday`, `2026-11-01` (also `/` or `.` between), `11-01` (this year's, or last
//! year's when it is still to come), `-3d` / `3d` (days ago), `-2w` / `2w` (weeks ago), a
//! weekday (`mon`, `monday`: the last one, today included). Case and spaces around are ignored;
//! anything else is no day.

use crate::time::{days, ymd};
use slakio_core::model::Ts;

/// How far back a count of days or weeks may reach.
const BACK_MAX: i64 = 36_500;

const WEEKDAYS: [&str; 7] = ["thursday", "friday", "saturday", "sunday", "monday", "tuesday", "wednesday"];

/// The day `text` names, `today` being the day it is (days since 1970-01-01); `None` when it
/// names none.
pub fn parse(text: &str, today: i64) -> Option<i64> {
    let t = text.trim().to_ascii_lowercase();
    match t.as_str() {
        "today" | "now" => return Some(today),
        "yesterday" => return Some(today - 1),
        _ => {}
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
            let (year, ..) = ymd(today);
            match days(year, m, d)? {
                day if day > today => days(year - 1, m, d),
                day => Some(day),
            }
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

/// The start of `day` as a Slack time.
pub fn start(day: i64) -> Ts {
    Ts(u64::try_from(day).unwrap_or(0) * 86_400 * 1_000_000)
}

#[cfg(test)]
mod tests;
