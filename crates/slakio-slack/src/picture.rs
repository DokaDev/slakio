//! A picture's bytes, read-only and never saved: a smaller copy of a picture a message shares,
//! from Slack's file host, or a person's profile photo, from an avatar host
//! ([`slakio_core::images::AVATAR_HOSTS`]). The UI decodes them.
//!
//! * Only an address its kind may come from is fetched ([`Source::checked`]), again here
//!   whatever the caller checked: a copy from the file host, a photo from an avatar host.
//! * A copy goes with the session's cookie (as the browser loads it on Slack's pages); a photo,
//!   and a workspace's own emoji (from the emoji host,
//!   [`slakio_core::images::EMOJI_HOSTS`]), with no cookie and no token ([`avatar_headers`]),
//!   and the wire refuses one that would.
//! * A copy follows a redirect (at most [`crate::download::REDIRECTS`]) only to another address
//!   on the file host; a photo (at most [`AVATAR_REDIRECTS`]) only to another address on an
//!   avatar host, checked again as a photo's address (`https`, the exact host, no port, no
//!   userinfo, a Gravatar fallback only Slack's) and asked the same way, with nothing of the
//!   session (when Gravatar sends a photo to any other host, the Slack default avatar its
//!   address already named as its fallback, checked the same way, is asked as the next hop
//!   instead: nothing is asked of a host not on the list); an emoji (at most [`EMOJI_REDIRECTS`]) only to another address on the emoji host,
//!   asked the same bare way. Each ask, and each hop, passes the kill switch and counts as one background
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

/// The most redirects an emoji's picture follows, each only to another address on the emoji
/// host.
pub const EMOJI_REDIRECTS: usize = 2;

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

/// The bytes of the picture at `source`, at most `max` of them, within [`DEADLINE`]; one for the
/// `list` keeps to its part of the pictures' share.
pub(crate) async fn picture(client: &Client, source: &Source, max: u64, list: bool) -> Result<Vec<u8>, SlackError> {
    let note = Note::new(client.diag(), source);
    let read = match tokio::time::timeout(DEADLINE, read(client, (source, list), max, &note)).await {
        Ok(read) => read,
        Err(_) => Err(SlackError::Network { detail: "the picture took too long".to_string() }),
    };
    if let Err(e) = &read {
        note.failed(e);
    }
    read
}

/// The picture diagnostics' lines of one picture ([`slakio_core::diag`]; nothing when off).
struct Note<'a> {
    diag: &'a slakio_core::diag::Diag,
    source: &'a Source,
    kind: slakio_core::diag::Kind,
    id: String,
}

impl<'a> Note<'a> {
    fn new(diag: &'a slakio_core::diag::Diag, source: &'a Source) -> Self {
        let id = if diag.on() { diag.id(source.address()) } else { String::new() };
        Self { diag, source, kind: slakio_core::diag::Kind::of(source), id }
    }

    fn line(&self, stage: slakio_core::diag::Stage, result: &str, pairs: &[(&str, &str)]) {
        let mut all = vec![("id", self.id.as_str())];
        all.extend_from_slice(pairs);
        self.diag.line(self.kind, stage, result, &all);
    }

    /// Hop `hop` asks `url` (for Gravatar, also where its fallback goes and whether it is taken).
    fn asked(&self, url: &str, hop: usize) {
        if !self.diag.on() {
            return;
        }
        let (host, hop) = (slakio_core::diag::host(url), hop.to_string());
        match slakio_core::diag::gravatar_fallback(url) {
            Some((d_host, ok)) => {
                let d_allowed = if ok { "yes" } else { "no" };
                let pairs = [("hop", hop.as_str()), ("host", &host), ("d_host", &d_host), ("d_allowed", d_allowed)];
                self.line(slakio_core::diag::Stage::Asked, "ok", &pairs);
            }
            None => self.line(slakio_core::diag::Stage::Asked, "ok", &[("hop", &hop), ("host", &host)]),
        }
    }

    /// Hop `hop` was answered `status`; a redirect's `location` is followed (`to`) or not; when
    /// `fallback`, it was refused and `to` is Gravatar's Slack fallback instead.
    fn answered(&self, hop: usize, status: u16, location: Option<&str>, to: Option<&str>, fallback: bool) {
        if !self.diag.on() {
            return;
        }
        let hop = hop.to_string();
        self.line(slakio_core::diag::Stage::Status, &status.to_string(), &[("hop", &hop)]);
        if let Some(location) = location.filter(|_| (300..400).contains(&status)) {
            let host = slakio_core::diag::host(location);
            let result = if to.is_some() && !fallback { "followed" } else { "refused" };
            self.line(slakio_core::diag::Stage::Redirected, result, &[("hop", &hop), ("host", &host)]);
        }
        if let Some(to) = to.filter(|_| fallback) {
            let host = slakio_core::diag::host(to);
            self.line(slakio_core::diag::Stage::Redirected, "fallback", &[("hop", &hop), ("host", &host)]);
        }
    }

    /// The answer says it is `content_type` (its `type/subtype` only).
    fn content_type(&self, content_type: Option<&str>) {
        if !self.diag.on() {
            return;
        }
        let t = content_type.map_or("none", |t| t.split(';').next().unwrap_or("").trim());
        self.line(slakio_core::diag::Stage::ContentType, &t.to_ascii_lowercase(), &[]);
    }

    /// The picture failed with `e`.
    fn failed(&self, e: &SlackError) {
        if !self.diag.on() {
            return;
        }
        match e {
            SlackError::Api { code } if code == "not_a_picture_address" => {
                let why = slakio_core::diag::refusal(self.source).unwrap_or("other");
                self.line(slakio_core::diag::Stage::AddressRefused, why, &[]);
            }
            SlackError::Api { code } => self.line(slakio_core::diag::Stage::Refused, code, &[]),
            SlackError::TooLarge => self.line(slakio_core::diag::Stage::Refused, "too-large", &[]),
            e => crate::shape::failed(self.diag, self.kind, e, &[("id", &self.id)]),
        }
    }
}

async fn read(
    client: &Client,
    (source, list): (&Source, bool),
    max: u64,
    note: &Note<'_>,
) -> Result<Vec<u8>, SlackError> {
    let mut url = source.checked().ok_or_else(|| api("not_a_picture_address"))?;
    let mut hops = 0;
    let mut answer = loop {
        note.asked(&url, hops);
        let answer = client.fetch_picture(&url, source, list).await?;
        let mut fallback = None;
        let to = match (answer.status, source) {
            (301 | 302 | 303 | 307 | 308, Source::Thumb(_)) => {
                answer.location.as_deref().and_then(crate::download::redirect)
            }
            (301 | 302 | 303 | 307 | 308, Source::Avatar(_)) => {
                let to = answer.location.as_deref().and_then(slakio_core::images::avatar_address);
                fallback = to.is_none().then(|| slakio_core::images::gravatar_fallback(&url)).flatten();
                to.or_else(|| fallback.clone())
            }
            (301 | 302 | 303 | 307 | 308, Source::Emoji(_)) => {
                answer.location.as_deref().and_then(slakio_core::images::emoji_address)
            }
            _ => None,
        };
        let most = match source {
            Source::Thumb(_) => crate::download::REDIRECTS,
            Source::Avatar(_) => AVATAR_REDIRECTS,
            Source::Emoji(_) => EMOJI_REDIRECTS,
        };
        let taken = to.as_deref().filter(|_| hops < most);
        note.answered(hops, answer.status, answer.location.as_deref(), taken, fallback.is_some());
        match answer.status {
            200 => break answer,
            301 | 302 | 303 | 307 | 308 if hops < most => {
                hops += 1;
                url = to.ok_or_else(|| api("redirected_elsewhere"))?;
            }
            301 | 302 | 303 | 307 | 308 => return Err(api("redirected_elsewhere")),
            429 => {
                return Err(SlackError::RateLimited {
                    retry_after: crate::response::retry_after(answer.retry_after.as_deref()),
                });
            }
            401 | 403 | 404 | 410 => return Err(api("file_not_available")),
            status => return Err(SlackError::Network { detail: format!("the picture host answered {status}") }),
        }
    };
    note.content_type(answer.content_type.as_deref());
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
