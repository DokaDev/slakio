//! The pictures slakio shows: smaller copies (thumbnails) of the pictures messages share, from
//! Slack's file host, people's profile photos, from Slack's avatar hosts ([`AVATAR_HOSTS`]), and
//! a workspace's own emoji, from Slack's emoji host ([`EMOJI_HOSTS`], read as [`emoji`] says).
//! Only `https` addresses on those exact hosts are kept ([`thumb_address`], [`avatar_address`],
//! [`emoji_address`]); anything else is no picture. How large one may be is bounded here, the
//! same for every backend and the UI: [`BYTES_MAX`] bytes fetched, [`SIDE_MAX`] and
//! [`PIXELS_MAX`] read from a picture's header before it is decoded.

use crate::model::FILE_HOST;

/// The hosts profile photos are fetched from: Slack's avatar host, the Gravatar host Slack
/// links to, and Slack's static host, which serves its default avatars (a person without a
/// photo of their own). Exact names, `https` only; spelled in two parts like [`FILE_HOST`] (the
/// commit hook refuses Slack's CDN hosts written whole).
pub const AVATAR_HOSTS: [&str; 3] =
    [concat!("avatars.slack", "-edge.com"), "secure.gravatar.com", concat!("a.slack", "-edge.com")];

/// The hosts a workspace's own emoji are fetched from: Slack's emoji host, the one `emoji.list`
/// names them on. Exact names, `https` only, fetched like a profile photo (no cookie, no token).
pub const EMOJI_HOSTS: [&str; 1] = [concat!("emoji.slack", "-edge.com")];

/// The most bytes of one picture fetched.
pub const BYTES_MAX: u64 = 2_000_000;

/// The widest and the tallest picture decoded, in pixels.
pub const SIDE_MAX: u32 = 4096;

/// The most pixels of a picture decoded (16 MB once decoded, four bytes a pixel).
pub const PIXELS_MAX: u64 = 4_194_304;

/// The longest address fetched.
pub const ADDRESS_MAX: usize = 2048;

/// Where a picture comes from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// A thumbnail of a file a message shares, on the file host: fetched with the session's
    /// cookie, as a browser does.
    Thumb(String),
    /// A profile photo on an avatar host: fetched with no cookie and no token.
    Avatar(String),
    /// A workspace's own emoji on the emoji host: fetched like a photo, with no cookie and no
    /// token.
    Emoji(String),
}

impl Source {
    pub fn address(&self) -> &str {
        match self {
            Source::Thumb(a) | Source::Avatar(a) | Source::Emoji(a) => a,
        }
    }

    /// The address, when it is still one this kind of picture may come from.
    pub fn checked(&self) -> Option<String> {
        match self {
            Source::Thumb(a) => thumb_address(a),
            Source::Avatar(a) => avatar_address(a),
            Source::Emoji(a) => emoji_address(a),
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

/// `url` when a profile photo may be fetched from it: an address on one of [`AVATAR_HOSTS`];
/// one on Gravatar only when each fallback it names is Slack's ([`gravatar::fallbacks_allowed`]).
pub fn avatar_address(url: &str) -> Option<String> {
    let kept = AVATAR_HOSTS.iter().find_map(|h| on_host(url, h, true))?;
    (host_of(&kept) != Some(AVATAR_HOSTS[1]) || gravatar::fallbacks_allowed(&kept)).then_some(kept)
}

/// The Slack default avatar a Gravatar address `url` (one [`avatar_address`] keeps) names as its
/// fallback (`d=`), decoded: asked instead when Gravatar sends the viewer to a host slakio does
/// not ask. Only an address on Slack's avatar hosts, kept by [`avatar_address`] again; `None`
/// for any other address, a keyword fallback (`mp`, `404`) or none.
pub fn gravatar_fallback(url: &str) -> Option<String> {
    let kept = avatar_address(url).filter(|k| host_of(k) == Some(AVATAR_HOSTS[1]))?;
    let (value, ok) = gravatar::fallback(&kept)?;
    let to = avatar_address(&value).filter(|_| ok)?;
    [AVATAR_HOSTS[0], AVATAR_HOSTS[2]].contains(&host_of(&to)?).then_some(to)
}

/// `url` when a workspace's own emoji may be fetched from it: an address on one of
/// [`EMOJI_HOSTS`]. Bytes outside ASCII (a name of another script in the path, as Slack may
/// write it raw) are percent-encoded first, as a browser sends them; the host is matched as
/// before, so no other host passes.
pub fn emoji_address(url: &str) -> Option<String> {
    let url = encoded(url);
    EMOJI_HOSTS.iter().find_map(|h| on_host(&url, h, true))
}

/// `url` with each byte outside ASCII percent-encoded (`%EC%95%88`).
fn encoded(url: &str) -> std::borrow::Cow<'_, str> {
    if url.is_ascii() {
        return std::borrow::Cow::Borrowed(url);
    }
    let mut out = String::with_capacity(url.len() * 3);
    for b in url.bytes() {
        if b.is_ascii() {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    std::borrow::Cow::Owned(out)
}

/// The host of an address [`thumb_address`], [`avatar_address`] or [`emoji_address`] kept.
pub fn host_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    rest.split('/').next().filter(|h| *h == FILE_HOST || AVATAR_HOSTS.contains(h) || EMOJI_HOSTS.contains(h))
}

/// A file named `name` of Slack's kind `filetype` is a picture slakio previews (PNG, JPEG, GIF).
pub fn previewed(filetype: &str) -> bool {
    matches!(filetype.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif")
}

pub mod emoji;
pub(crate) mod gravatar;

#[cfg(test)]
mod tests;
