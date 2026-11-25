//! How long a picture that failed for a while waits before it is asked again. A file preview
//! (the API's budget, the session's cookie) waits as it always has: [`super::RETRY_SOON`] for a
//! busy or offline backend, [`super::RETRY_AFTER`] for the network or the request budget. A
//! profile photo or an emoji (the picture hosts' own budget) waits [`CDN_BACKOFF`] in turn, its
//! first, second, then third and every later try: those hosts take a screen's worth in seconds,
//! so a minute's wait would leave it text for nothing. A host that said how long to wait (429)
//! is waited for that long when it is longer, at most an hour. What cannot be shown (gone, not a
//! picture, too large) is never asked again.

use super::{RETRY_AFTER, RETRY_SOON};
use slakio_core::backend::FailKind;
use slakio_core::images::Source;
use std::time::Duration;

/// The waits of a photo or an emoji that failed for a while, try after try (the last repeats).
pub const CDN_BACKOFF: [Duration; 3] = [Duration::from_secs(2), Duration::from_secs(5), Duration::from_secs(15)];

/// The longest a picture host's own word (`Retry-After`) is waited for.
const SAID_MAX: Duration = Duration::from_secs(3600);

/// How long a picture of `source` that failed for `kind` waits before it is asked again, on its
/// `tries`-th failure in a row (1 the first); `None`: never.
pub(super) fn wait(source: &Source, kind: &FailKind, tries: u8) -> Option<Duration> {
    if let Source::Thumb(_) = source {
        return match kind {
            FailKind::Busy | FailKind::Offline => Some(RETRY_SOON),
            k if k.retryable() || *k == FailKind::CapReached => Some(RETRY_AFTER),
            _ => None,
        };
    }
    let step = CDN_BACKOFF[usize::from(tries.saturating_sub(1)).min(CDN_BACKOFF.len() - 1)];
    match kind {
        FailKind::CapReached | FailKind::Busy | FailKind::Offline | FailKind::Network | FailKind::Stalled => Some(step),
        FailKind::RateLimited { retry_after } => Some(retry_after.unwrap_or_default().min(SAID_MAX).max(step)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
