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
    refusal(url).is_none()
}

/// Why [`fallbacks_allowed`] refuses `url`, in a word (`semicolon`, `param`, `escape`,
/// `d-host`); `None` when it does not.
pub(crate) fn refusal(url: &str) -> Option<&'static str> {
    let (_, query) = url.split_once('?')?;
    if query.contains(';') {
        return Some("semicolon");
    }
    query.split('&').filter(|pair| !pair.is_empty()).find_map(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let Some(key) = decode(key) else { return Some("escape") };
        if key.trim() != key || key.contains('\0') || !NAMES.iter().any(|n| key.eq_ignore_ascii_case(n)) {
            return Some("param");
        }
        if !fallback_key(&key) {
            return None;
        }
        let Some(value) = decode(value) else { return Some("escape") };
        (!fallback_ok(&value)).then_some("d-host")
    })
}

/// `key` names a fallback (`d`, `default`).
fn fallback_key(key: &str) -> bool {
    key.eq_ignore_ascii_case("d") || key.eq_ignore_ascii_case("default")
}

/// A decoded fallback `value` is a keyword or an address on Slack's avatar hosts.
fn fallback_ok(value: &str) -> bool {
    value.bytes().all(|b| b.is_ascii_alphanumeric())
        || [AVATAR_HOSTS[0], AVATAR_HOSTS[2]].iter().any(|h| on_host(value, h, true).is_some())
}

/// The first fallback `url` names, decoded, and whether it is one [`fallbacks_allowed`] takes;
/// `None` when it names none (or its escape is broken).
pub(crate) fn fallback(url: &str) -> Option<(String, bool)> {
    let (_, query) = url.split_once('?')?;
    query.split(['&', ';']).find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        let value = decode(value).filter(|_| decode(key).is_some_and(|k| fallback_key(&k)))?;
        let ok = fallback_ok(&value);
        Some((value, ok))
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
