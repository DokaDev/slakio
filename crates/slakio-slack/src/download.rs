//! Downloading a file a message shares, read-only: a `GET` of its address on Slack's file host
//! ([`FILE_HOST`]), the session's cookie sent there and nowhere else, saved as
//! [`slakio_core::download`] says.
//!
//! * Only an address on the file host is fetched ([`address`]): `https`, that exact host, a
//!   plain path; anything else is no download.
//! * A redirect is followed (at most [`REDIRECTS`]) only to another address on the file host
//!   (or a path on it); one to any other host fails without going there, so the cookie never
//!   leaves the file host. Each step passes the kill switch and counts as a request.
//! * A file is refused before it is asked for when it says it is larger than `max`, and when
//!   the answer says so; the bytes are counted as they come, and the download stops at `max`.
//! * An answer that is a web page for a file that is not one (a sign-in page) is not saved.
//! * How much has come is told at most every [`PROGRESS_BYTES`] or [`PROGRESS_EVERY`].

use crate::client::Client;
use crate::error::SlackError;
use crate::request::Os;
use slakio_core::download::{Saving, Unsaved};
use slakio_core::model::{FILE_HOST, File};
use slakio_core::secret::Credentials;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The most redirects a download follows.
pub const REDIRECTS: usize = 3;

/// How often progress is told, at most: after this many bytes, or this long.
const PROGRESS_BYTES: u64 = 256 * 1024;
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// The longest address fetched.
const ADDRESS_MAX: usize = 2048;

/// `url` when it is an address a file may be downloaded from: `https://<FILE_HOST>/…`, the
/// path plain printable ASCII (no space, no fragment, no `\`), at most [`ADDRESS_MAX`] bytes.
pub fn address(url: &str) -> Option<String> {
    let path = url.strip_prefix("https://")?.strip_prefix(FILE_HOST)?;
    let plain = path.starts_with('/')
        && url.len() <= ADDRESS_MAX
        && path.bytes().all(|b| b.is_ascii_graphic() && b != b'#' && b != b'\\');
    plain.then(|| url.to_string())
}

/// Where redirect `location` goes, when it may be fetched: an address on the file
/// host, or a path on it.
fn redirect(location: &str) -> Option<String> {
    if location.starts_with('/') && !location.starts_with("//") {
        return address(&format!("https://{FILE_HOST}{location}"));
    }
    address(location)
}

/// The headers of a file's download, in order: the browser's as it follows a download link
/// from the web client, and the session's cookie (never the token).
pub(crate) fn headers(credentials: &Credentials, os: Os) -> Vec<(&'static str, String)> {
    let cookie = match credentials.cookie_ds() {
        Some(ds) => format!("d={}; d-s={ds}", credentials.cookie()),
        None => format!("d={}", credentials.cookie()),
    };
    let mut h = crate::request::browser_headers(os);
    // A download is a navigation, not an API call: no `Origin`, the page it came from as the
    // `Referer`.
    h.retain(|(n, _)| !matches!(*n, "Origin" | "Sec-Fetch-Mode" | "Sec-Fetch-Dest" | "Priority"));
    h.push(("Sec-Fetch-Mode", "navigate".to_string()));
    h.push(("Sec-Fetch-Dest", "document".to_string()));
    h.push(("Referer", "https://app.slack.com/".to_string()));
    h.push(("Cookie", cookie));
    h
}

/// The answer is a web page where a file that is not one was asked for (a sign-in page).
fn a_page_instead(content_type: Option<&str>, file: &File) -> bool {
    let page = content_type.is_some_and(|t| t.trim().to_ascii_lowercase().starts_with("text/html"));
    let name = file.name.unsanitized().to_ascii_lowercase();
    page && !(name.ends_with(".html") || name.ends_with(".htm"))
}

/// Download `file` with `client` into `dir`, at most `max` bytes, telling `progress` how much
/// has come (and of how much, when said). The path it is saved as, and its length.
pub(crate) async fn download(
    client: &Client,
    file: &File,
    dir: &Path,
    max: u64,
    progress: impl Fn(u64, Option<u64>),
) -> Result<(PathBuf, u64), SlackError> {
    let mut url = file.download.as_deref().and_then(address).ok_or_else(|| api("file_not_downloadable"))?;
    if file.size.is_some_and(|s| s > max) {
        return Err(SlackError::TooLarge);
    }
    let mut hops = 0;
    let mut answer = loop {
        let answer = client.fetch_file(&url).await?;
        match answer.status {
            200 => break answer,
            301 | 302 | 303 | 307 | 308 if hops < REDIRECTS => {
                hops += 1;
                let to = answer.location.as_deref().and_then(redirect);
                url = to.ok_or_else(|| api("redirected_elsewhere"))?;
            }
            301 | 302 | 303 | 307 | 308 => return Err(api("too_many_redirects")),
            429 => {
                return Err(SlackError::RateLimited {
                    retry_after: crate::response::retry_after(answer.retry_after.as_deref()),
                });
            }
            401 | 403 | 404 | 410 => return Err(api("file_not_available")),
            status => return Err(SlackError::Network { detail: format!("the file host answered {status}") }),
        }
    };
    if a_page_instead(answer.content_type.as_deref(), file) {
        return Err(api("not_a_file"));
    }
    if answer.length.is_some_and(|l| l > max) {
        return Err(SlackError::TooLarge);
    }
    let total = answer.length.or(file.size);
    let mut saving = Saving::start(dir, &file.name, max).map_err(unsaved)?;
    let (mut told, mut told_at) = (0u64, Instant::now());
    progress(0, total);
    loop {
        let chunk = tokio::select! {
            biased;
            () = client.governor().stopped() => return Err(SlackError::Offline { why: "the network was switched off" }),
            chunk = answer.chunk() => chunk?,
        };
        let Some(bytes) = chunk else { break };
        saving.write(&bytes).map_err(unsaved)?;
        let got = saving.written();
        if got - told >= PROGRESS_BYTES || told_at.elapsed() >= PROGRESS_EVERY {
            (told, told_at) = (got, Instant::now());
            progress(got, total);
        }
    }
    let bytes = saving.written();
    // Cut off before the length said: not the whole file.
    if answer.length.is_some_and(|l| l != bytes) {
        return Err(SlackError::Network { detail: "the download was cut off".to_string() });
    }
    let path = saving.finish().map_err(unsaved)?;
    Ok((path, bytes))
}

fn api(code: &str) -> SlackError {
    SlackError::Api { code: code.to_string() }
}

fn unsaved(e: Unsaved) -> SlackError {
    match e {
        Unsaved::TooLarge => SlackError::TooLarge,
        Unsaved::Io(e) => SlackError::Unsaved { detail: format!("{:?}", e.kind()) },
    }
}

#[cfg(test)]
mod tests;
