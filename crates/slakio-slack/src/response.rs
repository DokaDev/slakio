//! Slack's answers: an HTTP status and a JSON body with `ok` and, when it is `false`, an error
//! `code`. Slack answers 200 even to a request it refuses, so any other status is a transport
//! or proxy failure (its body is often an HTML page) and is never parsed.

use crate::error::SlackError;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::time::Duration;

/// An answer as it came off the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raw {
    pub status: u16,
    /// The `Retry-After` header, when there was one.
    pub retry_after: Option<String>,
    pub body: Vec<u8>,
}

/// The codes that mean the sign-in is not (or no longer) accepted.
const AUTH_CODES: &[&str] =
    &["invalid_auth", "not_authed", "account_inactive", "token_revoked", "token_expired", "user_removed_from_team"];

#[derive(Deserialize)]
struct Envelope {
    ok: Option<bool>,
    error: Option<String>,
}

/// The body of `raw` as `T`, or why there is none.
pub fn decode<T: DeserializeOwned>(raw: &Raw) -> Result<T, SlackError> {
    if raw.status == 429 {
        return Err(SlackError::RateLimited { retry_after: retry_after(raw.retry_after.as_deref()) });
    }
    if !(200..300).contains(&raw.status) {
        return Err(SlackError::Network { detail: format!("HTTP {}", raw.status) });
    }
    let malformed = || SlackError::Network { detail: "the answer is not Slack's JSON".to_string() };
    let envelope: Envelope = serde_json::from_slice(&raw.body).map_err(|_| malformed())?;
    match envelope.ok {
        Some(true) => serde_json::from_slice(&raw.body).map_err(|_| malformed()),
        Some(false) => Err(api_error(envelope.error.unwrap_or_else(|| "no_error_code".to_string()))),
        None => Err(malformed()),
    }
}

/// Slack's error `code`, as the transport tells it apart.
fn api_error(code: String) -> SlackError {
    // A code is Slack's word, but it reaches the error log: kept only when it is one (short,
    // `snake_case`), never what else a server put there.
    let plain = code.len() <= 64 && code.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    let code = if plain && !code.is_empty() { code } else { "not_a_slack_code".to_string() };
    if AUTH_CODES.contains(&code.as_str()) {
        SlackError::Auth { code }
    } else if code == "ratelimited" {
        SlackError::RateLimited { retry_after: None }
    } else {
        SlackError::Api { code }
    }
}

/// The longest pause a `Retry-After` is taken for: a larger one is read as this.
pub(crate) const RETRY_AFTER_MAX: Duration = Duration::from_secs(60 * 60);

/// A `Retry-After` of whole seconds, at most [`RETRY_AFTER_MAX`]; `None` when absent or in
/// another form (unknown is not a default).
pub(crate) fn retry_after(header: Option<&str>) -> Option<Duration> {
    header?.trim().parse::<u64>().ok().map(|s| Duration::from_secs(s).min(RETRY_AFTER_MAX))
}

#[cfg(test)]
mod tests;
