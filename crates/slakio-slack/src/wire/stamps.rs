//! Slack's times as they come in its answers.

/// An `updated` time of Slack's (milliseconds since the Unix epoch, a number or its text) as
/// microseconds; `None` when it is not one of a plausible size (from 2001 to 2286).
pub fn updated(v: &serde_json::Value) -> Option<u64> {
    let ms = v.as_u64().or_else(|| v.as_str()?.parse().ok())?;
    (1_000_000_000_000..10_000_000_000_000).contains(&ms).then(|| ms * 1000)
}

/// A Slack timestamp (`1767603600.000100`) as microseconds; `None` when it is not one.
pub fn ts(s: &str) -> Option<u64> {
    let (secs, micros) = s.split_once('.').unwrap_or((s, "0"));
    if micros.len() > 6 || !micros.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let micros: u64 = format!("{micros:0<6}").parse().ok()?;
    secs.parse::<u64>().ok()?.checked_mul(1_000_000)?.checked_add(micros)
}
