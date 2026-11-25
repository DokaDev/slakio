//! Slack's times as they come in its answers.

/// A Slack timestamp (`1767603600.000100`) as microseconds; `None` when it is not one.
pub fn ts(s: &str) -> Option<u64> {
    let (secs, micros) = s.split_once('.').unwrap_or((s, "0"));
    if micros.len() > 6 || !micros.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let micros: u64 = format!("{micros:0<6}").parse().ok()?;
    secs.parse::<u64>().ok()?.checked_mul(1_000_000)?.checked_add(micros)
}
