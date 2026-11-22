//! A picture's bytes, read-only and never saved: a smaller copy of a picture a message shares,
//! from Slack's file host, or a person's profile photo, from an avatar host
//! ([`slakio_core::images::AVATAR_HOSTS`]). The UI decodes them.
//!
//! * Only an address its kind may come from is fetched ([`Source::checked`]), again here
//!   whatever the caller checked: a copy from the file host, a photo from an avatar host.
//! * A copy goes with the session's cookie (as the browser loads it on Slack's pages); a photo
//!   with no cookie and no token ([`avatar_headers`]), and the wire refuses one that would.
//! * A copy follows a redirect (at most [`crate::download::REDIRECTS`]) only to another address
//!   on the file host; a photo (at most [`AVATAR_REDIRECTS`]) only to another address on an
//!   avatar host, checked again as a photo's address (`https`, the exact host, no port, no
//!   userinfo, a Gravatar fallback only Slack's) and asked the same way, with nothing of the
//!   session. Each ask, and each hop, passes the kill switch and counts as one background
//!   request of the workspace.
//! * Only an answer that says it is an image (`image/…`) is read, at most `max` bytes of it:
//!   a larger one, said or counted, is refused; all of it within [`DEADLINE`].

use crate::client::Client;
use crate::error::SlackError;
use crate::request::Os;
use slakio_core::images::Source;
use slakio_core::secret::Credentials;
use std::time::Duration;

/// The most redirects a profile photo follows (Gravatar to Slack's default avatar is one).
pub const AVATAR_REDIRECTS: usize = 2;

/// The longest one picture may take, from asking to its last byte, however its bytes come (one
/// that keeps a host's answer trickling never holds a place for longer).
pub const DEADLINE: Duration = Duration::from_secs(25);

fn api(code: &str) -> SlackError {
    SlackError::Api { code: code.to_string() }
}

/// The headers a copy on the file host is asked for with, in order: the browser's as it loads
/// an image on Slack's pages, and the session's cookie (never the token).
pub(crate) fn thumb_headers(credentials: &Credentials, os: Os) -> Vec<(&'static str, String)> {
    let mut h = crate::download::headers(credentials, os);
    for (name, value) in &mut h {
        match *name {
            "Accept" => *value = IMAGES.to_string(),
            "Sec-Fetch-Mode" => *value = "no-cors".to_string(),
            "Sec-Fetch-Dest" => *value = "image".to_string(),
            _ => {}
        }
    }
    h
}

/// What an image is asked for as.
const IMAGES: &str = "image/avif,image/webp,image/apng,image/*,*/*;q=0.8";

/// The headers a profile photo is asked for with: a browser's image request, nothing of the
/// session (no cookie, no token, no `Origin`, no `Referer`).
pub(crate) fn avatar_headers(os: Os) -> Vec<(&'static str, String)> {
    vec![
        ("User-Agent", os.user_agent()),
        ("Accept", IMAGES.to_string()),
        ("Accept-Language", "en-US,en;q=0.9".to_string()),
        ("Sec-Fetch-Site", "cross-site".to_string()),
        ("Sec-Fetch-Mode", "no-cors".to_string()),
        ("Sec-Fetch-Dest", "image".to_string()),
    ]
}

/// The bytes of the picture at `source`, at most `max` of them, within [`DEADLINE`].
pub(crate) async fn picture(client: &Client, source: &Source, max: u64) -> Result<Vec<u8>, SlackError> {
    match tokio::time::timeout(DEADLINE, read(client, source, max)).await {
        Ok(read) => read,
        Err(_) => Err(SlackError::Network { detail: "the picture took too long".to_string() }),
    }
}

async fn read(client: &Client, source: &Source, max: u64) -> Result<Vec<u8>, SlackError> {
    let mut url = source.checked().ok_or_else(|| api("not_a_picture_address"))?;
    let mut hops = 0;
    let mut answer = loop {
        let answer = client.fetch_picture(&url, source).await?;
        match (answer.status, source) {
            (200, _) => break answer,
            (301 | 302 | 303 | 307 | 308, Source::Thumb(_)) if hops < crate::download::REDIRECTS => {
                hops += 1;
                let to = answer.location.as_deref().and_then(crate::download::redirect);
                url = to.ok_or_else(|| api("redirected_elsewhere"))?;
            }
            (301 | 302 | 303 | 307 | 308, Source::Avatar(_)) if hops < AVATAR_REDIRECTS => {
                hops += 1;
                let to = answer.location.as_deref().and_then(slakio_core::images::avatar_address);
                url = to.ok_or_else(|| api("redirected_elsewhere"))?;
            }
            (301 | 302 | 303 | 307 | 308, _) => return Err(api("redirected_elsewhere")),
            (429, _) => {
                return Err(SlackError::RateLimited {
                    retry_after: crate::response::retry_after(answer.retry_after.as_deref()),
                });
            }
            (401 | 403 | 404 | 410, _) => return Err(api("file_not_available")),
            (status, _) => return Err(SlackError::Network { detail: format!("the picture host answered {status}") }),
        }
    };
    let image = answer.content_type.as_deref().is_some_and(|t| t.trim().to_ascii_lowercase().starts_with("image/"));
    if !image {
        return Err(api("not_an_image"));
    }
    if answer.length.is_some_and(|l| l > max) {
        return Err(SlackError::TooLarge);
    }
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::select! {
            biased;
            () = client.governor().stopped() => return Err(SlackError::Offline { why: "the network was switched off" }),
            chunk = answer.chunk() => chunk?,
        };
        let Some(chunk) = chunk else { break };
        if (bytes.len() + chunk.len()) as u64 > max {
            return Err(SlackError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    if answer.length.is_some_and(|l| l != bytes.len() as u64) {
        return Err(SlackError::Network { detail: "the picture was cut off".to_string() });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
