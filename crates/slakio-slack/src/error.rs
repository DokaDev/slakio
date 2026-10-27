//! Why a call failed, as the transport tells it apart, and how the UI hears of it
//! ([`SlackError::kind`]): the core's [`FailKind`], plus a detail for the error log that never
//! holds a token, a cookie or a message.

use slakio_core::backend::FailKind;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlackError {
    /// The method is not on the allowlist: refused before anything was sent.
    NotAllowed { method: String },
    /// Signed out, or the sign-in is no longer accepted (Slack's `code`).
    Auth { code: String },
    /// Slack asked to slow down (HTTP 429, or `ratelimited`), for `retry_after` when it said.
    RateLimited { retry_after: Option<Duration> },
    /// The request did not get a usable answer (connection, timeout, a non-2xx status, a body
    /// that is not Slack's JSON).
    Network { detail: String },
    /// Nothing was sent: the network is switched off, or the address is one this version never
    /// reaches.
    Offline { why: &'static str },
    /// More calls wait for a free slot than the transport queues: nothing was sent.
    Busy,
    /// The request cap is reached: nothing was sent.
    CapReached { why: &'static str },
    /// Slack answered `ok: false` with `code`.
    Api { code: String },
}

impl SlackError {
    /// The core's words for it.
    pub fn kind(&self) -> FailKind {
        match self {
            SlackError::NotAllowed { .. } => FailKind::ReadOnly,
            SlackError::Auth { .. } => FailKind::Auth,
            SlackError::RateLimited { retry_after } => FailKind::RateLimited { retry_after: *retry_after },
            SlackError::Network { .. } => FailKind::Network,
            SlackError::Offline { .. } => FailKind::Offline,
            SlackError::Busy => FailKind::Busy,
            SlackError::CapReached { .. } => FailKind::CapReached,
            SlackError::Api { code } => FailKind::Api { code: code.clone() },
        }
    }

    /// One line for the error log: method names, Slack's error codes, statuses and fixed
    /// words only.
    pub fn detail(&self) -> String {
        match self {
            SlackError::NotAllowed { method } => format!("method not allowed: {method}"),
            SlackError::Auth { code } | SlackError::Api { code } => code.clone(),
            SlackError::RateLimited { retry_after: Some(d) } => format!("retry after {}s", d.as_secs()),
            SlackError::RateLimited { retry_after: None } => "rate limited".to_string(),
            SlackError::Network { detail } => detail.clone(),
            SlackError::Offline { why } | SlackError::CapReached { why } => (*why).to_string(),
            SlackError::Busy => "too many requests waiting".to_string(),
        }
    }
}
