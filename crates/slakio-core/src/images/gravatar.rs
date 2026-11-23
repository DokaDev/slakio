//! Gravatar's fallback: an address on Gravatar may name, in `d=` (or `default=`), where Gravatar
//! sends the viewer when the person has no Gravatar picture. Slack names its own default avatar
//! there, on its static host. Such an address is kept only when every fallback it names is a
//! keyword of Gravatar's own pictures (letters and digits only: `mp`, `identicon`, `404`) or,
//! once percent-decoded, an address on one of Slack's avatar hosts; any other (another host,
//! Gravatar itself, `http`, a path, a broken escape) refuses the whole address, so nothing is
//! asked of Gravatar that would send the viewer elsewhere. Only Gravatar's own parameters may
//! stand in the query, named plainly, so no server reads a fallback slakio did not check.

use super::{AVATAR_HOSTS, on_host};

/// The parameters a Gravatar address may carry (size, fallback, rating, forced fallback), by
/// name; any other refuses the address.
const NAMES: [&str; 8] = ["s", "size", "d", "default", "r", "rating", "f", "forcedefault"];

/// Each fallback `url` (an address on Gravatar) names is a keyword or an address on Slack's
/// avatar hosts. A parameter's name is read decoded and without regard to case, as a server may;
/// it must be one of [`NAMES`] exactly (not padded with spaces, no NUL), and no `;` may stand in
/// the query (some servers split there too).
pub(super) fn fallbacks_allowed(url: &str) -> bool {
    let Some((_, query)) = url.split_once('?') else { return true };
    if query.contains(';') {
        return false;
    }
    query.split('&').filter(|pair| !pair.is_empty()).all(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let Some(key) = decode(key) else { return false };
        if key.trim() != key || key.contains('\0') || !NAMES.iter().any(|n| key.eq_ignore_ascii_case(n)) {
            return false;
        }
        if !key.eq_ignore_ascii_case("d") && !key.eq_ignore_ascii_case("default") {
            return true;
        }
        let Some(value) = decode(value) else { return false };
        value.bytes().all(|b| b.is_ascii_alphanumeric())
            || [AVATAR_HOSTS[0], AVATAR_HOSTS[2]].iter().any(|h| on_host(&value, h, true).is_some())
    })
}

/// `s` with its `%XX` escapes (and `+` as a space) decoded; `None` when an escape is broken or
/// the result is not UTF-8.
fn decode(s: &str) -> Option<String> {
    let mut out = Vec::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        match b {
            b'%' => {
                let hex = [bytes.next()?, bytes.next()?];
                // Two hex digits exactly: `%+F` is no escape.
                if !hex.iter().all(u8::is_ascii_hexdigit) {
                    return None;
                }
                out.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
            }
            b'+' => out.push(b' '),
            _ => out.push(b),
        }
    }
    String::from_utf8(out).ok()
}
