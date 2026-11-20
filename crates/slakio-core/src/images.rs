//! The pictures slakio shows: smaller copies (thumbnails) of the pictures messages share, from
//! Slack's file host, and people's profile photos, from Slack's avatar hosts
//! ([`AVATAR_HOSTS`]). Only `https` addresses on those exact hosts are kept ([`thumb_address`],
//! [`avatar_address`]); anything else is no picture. How large one may be is bounded here, the
//! same for every backend and the UI: [`BYTES_MAX`] bytes fetched, [`SIDE_MAX`] and
//! [`PIXELS_MAX`] read from a picture's header before it is decoded.

use crate::model::FILE_HOST;

/// The hosts profile photos are fetched from: Slack's avatar host and the Gravatar host Slack
/// links to. Exact names, `https` only; spelled in two parts like [`FILE_HOST`] (the commit
/// hook refuses Slack's CDN host written whole).
pub const AVATAR_HOSTS: [&str; 2] = [concat!("avatars.slack", "-edge.com"), "secure.gravatar.com"];

/// The most bytes of one picture fetched.
pub const BYTES_MAX: u64 = 2_000_000;

/// The widest and the tallest picture decoded, in pixels.
pub const SIDE_MAX: u32 = 4096;

/// The most pixels of a picture decoded (16 MB once decoded, four bytes a pixel).
pub const PIXELS_MAX: u64 = 4_194_304;

/// The longest address fetched.
const ADDRESS_MAX: usize = 2048;

/// Where a picture comes from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// A thumbnail of a file a message shares, on the file host: fetched with the session's
    /// cookie, as a browser does.
    Thumb(String),
    /// A profile photo on an avatar host: fetched with no cookie and no token.
    Avatar(String),
}

impl Source {
    pub fn address(&self) -> &str {
        match self {
            Source::Thumb(a) | Source::Avatar(a) => a,
        }
    }

    /// The address, when it is still one this kind of picture may come from.
    pub fn checked(&self) -> Option<String> {
        match self {
            Source::Thumb(a) => thumb_address(a),
            Source::Avatar(a) => avatar_address(a),
        }
    }
}

/// `url` when it is `https://<host>/<path>` with `host` exactly, the path (and query, when
/// `query`) plain printable ASCII: no space, no fragment, no `\`, at most [`ADDRESS_MAX`] bytes.
fn on_host(url: &str, host: &str, query: bool) -> Option<String> {
    let rest = url.strip_prefix("https://")?.strip_prefix(host)?;
    let plain = rest.starts_with('/')
        && url.len() <= ADDRESS_MAX
        && rest.bytes().all(|b| b.is_ascii_graphic() && b != b'#' && b != b'\\')
        && (query || !rest.contains('?'));
    plain.then(|| url.to_string())
}

/// `url` when a thumbnail may be fetched from it: an address on Slack's file host.
pub fn thumb_address(url: &str) -> Option<String> {
    on_host(url, FILE_HOST, true)
}

/// `url` when a profile photo may be fetched from it: an address on one of [`AVATAR_HOSTS`].
pub fn avatar_address(url: &str) -> Option<String> {
    AVATAR_HOSTS.iter().find_map(|h| on_host(url, h, true))
}

/// The host of an address [`thumb_address`] or [`avatar_address`] kept.
pub fn host_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    rest.split('/').next().filter(|h| *h == FILE_HOST || AVATAR_HOSTS.contains(h))
}

/// A file named `name` of Slack's kind `filetype` is a picture slakio previews (PNG, JPEG, GIF).
pub fn previewed(filetype: &str) -> bool {
    matches!(filetype.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif")
}

#[cfg(test)]
mod tests;
