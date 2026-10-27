//! A browser request pasted as cURL: the developer tools' "Copy as cURL" of any request the
//! Slack web app sends to its workspace (`https://<workspace>.slack.com/api/…`). The copy holds
//! what signs the browser in: the session token (in the form body) and the `d` cookie (and
//! `d-s`, when the browser has it), next to the workspace's address.
//!
//! The parser does not run or interpret the command: it looks for those pieces wherever the
//! browser put them, so the shapes of every browser's copy work — Chrome's and Edge's (`-H
//! 'cookie: …'` or `-b '…'`, a body in `$'…'` with escapes), Firefox's and Safari's (`-H 'Cookie:
//! …'`, `--data-raw` / `--data-binary`), the Windows `cmd` copy (`^"…^"`) and the PowerShell copy
//! (`New-Object System.Net.Cookie("d", "…")`). It is pure: nothing is sent, stored or printed.
//!
//! Errors say what is missing in fixed words and never carry any of the pasted text, so no
//! error, log line or panic message can hold a token or a cookie.

use slakio_core::secret::Credentials;
use std::fmt;

/// The most text read: a copy is a few kilobytes; anything far longer is not one.
pub const PASTE_MAX: usize = 256 * 1024;

/// The prefix of a browser session token, and of the `d` cookie's value (spelled in two parts
/// so that no file holds a token-shaped string).
const TOKEN_PREFIX: &str = concat!("xo", "xc-");
const COOKIE_PREFIX: &str = concat!("xo", "xd-");

/// Host names under `slack.com` that are Slack's own, never a workspace's.
const NOT_WORKSPACES: &[&str] = &[
    "app",
    "api",
    "edgeapi",
    "enterprise",
    "files",
    "status",
    "www",
    "slack",
    "my",
    "join",
    "downloads",
    "wss-primary",
    "wss-backup",
    "wss-mobile-primary",
    "wss-mobile-backup",
];

/// What a pasted copy signs in with, and where: the workspace's host (`example.slack.com`).
/// Its `Debug` print leaves the credentials out.
#[derive(Clone, PartialEq, Eq)]
pub struct Pasted {
    pub host: String,
    pub credentials: Credentials,
}

impl fmt::Debug for Pasted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pasted").field("host", &self.host).finish_non_exhaustive()
    }
}

/// Why a paste cannot sign in. None of them carries any of the pasted text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteError {
    /// Nothing was pasted.
    Empty,
    /// Far longer than any copy of a request.
    TooLong,
    /// No `https://<workspace>.slack.com` address.
    NoAddress,
    /// Slack's address, but not a workspace's (`app.slack.com`, `edgeapi.slack.com`, …).
    NotWorkspace,
    /// No browser session token.
    NoToken,
    /// No `d` cookie (a copy made without cookies, or from somewhere other than the Network tab).
    NoCookie,
    /// Two different tokens or two different `d` cookies: not one request's copy.
    Ambiguous,
}

impl fmt::Display for PasteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PasteError::Empty => "nothing was pasted",
            PasteError::TooLong => "the paste is too long to be one request",
            PasteError::NoAddress => "the paste has no workspace address (https://<workspace>.slack.com)",
            PasteError::NotWorkspace => "the request does not go to a workspace address",
            PasteError::NoToken => "the paste has no session token",
            PasteError::NoCookie => "the paste has no d cookie",
            PasteError::Ambiguous => "the paste holds more than one sign-in",
        })
    }
}

impl std::error::Error for PasteError {}

/// The workspace and the credentials in `text`, a pasted "Copy as cURL".
pub fn parse(text: &str) -> Result<Pasted, PasteError> {
    if text.len() > PASTE_MAX {
        return Err(PasteError::TooLong);
    }
    if text.trim().is_empty() {
        return Err(PasteError::Empty);
    }
    // The Windows `cmd` copy escapes with carets (`^"`, `^%`, `^&`, `^^`).
    let text = if text.contains("^\"") { uncaret(text) } else { text.to_string() };
    let host = address(&text)?;
    let mut steps = 0;
    let token = one(values(&text, TOKEN_PREFIX, is_token_char, &mut steps)?, PasteError::NoToken)?;
    let cookie = one(
        values(&text, COOKIE_PREFIX, is_cookie_char, &mut steps)?.into_iter().map(|v| wire(&v)).collect(),
        PasteError::NoCookie,
    )?;
    let mut credentials = Credentials::new(token, cookie);
    if let Some(ds) = cookie_ds(&text) {
        credentials = credentials.with_ds(ds);
    }
    Ok(Pasted { host, credentials })
}

/// The one value of `found`: `missing` when there is none, [`PasteError::Ambiguous`] when there
/// are two different ones.
fn one(mut found: Vec<String>, missing: PasteError) -> Result<String, PasteError> {
    found.dedup();
    let first = found.first().ok_or(missing)?;
    if found.iter().any(|v| v != first) {
        return Err(PasteError::Ambiguous);
    }
    Ok(first.clone())
}

/// `text` without the `cmd` escape carets: `^x` is `x` (so `^^` is `^`).
fn uncaret(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '^' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The first workspace host of the `https://` addresses in `text`, lowercased.
fn address(text: &str) -> Result<String, PasteError> {
    let mut slack = false;
    for (i, _) in text.match_indices("https://") {
        let rest = &text[i + "https://".len()..];
        let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-')).unwrap_or(rest.len());
        let host = rest[..end].trim_end_matches('.').to_ascii_lowercase();
        if host == "slack.com" || host.ends_with(".slack.com") {
            slack = true;
            if workspace_host(&host) {
                return Ok(host);
            }
        }
    }
    Err(if slack { PasteError::NotWorkspace } else { PasteError::NoAddress })
}

/// `host` is a workspace's: `<name>.slack.com` or `<name>.enterprise.slack.com`, `<name>` one
/// label of letters, digits and inner hyphens that is not one of Slack's own.
pub fn workspace_host(host: &str) -> bool {
    let name = host.strip_suffix(".enterprise.slack.com").or_else(|| host.strip_suffix(".slack.com"));
    name.is_some_and(|n| {
        !n.is_empty()
            && n.len() <= 63
            && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !n.starts_with('-')
            && !n.ends_with('-')
            && !NOT_WORKSPACES.contains(&n)
    })
}

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

/// What a cookie value may hold here: its percent-encoded form, and the characters of the
/// decoded one (a copy that decoded it).
fn is_cookie_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '%' | '-' | '_' | '.' | '~' | '/' | '+' | '=')
}

/// How many distinct values of one kind a copy may hold before it is not one request's.
const CANDIDATES_MAX: usize = 8;

/// The longest value read; a longer run is not a token or a cookie.
const VALUE_MAX: usize = 4096;

/// Every distinct value in `text` that starts with `prefix`, as far as `keep` holds, in order;
/// one too short or too long to be real is left out. One pass: the scan goes on after each
/// value, never back into it. More than [`CANDIDATES_MAX`] distinct values are
/// [`PasteError::Ambiguous`]. `steps` counts the characters looked at.
fn values(text: &str, prefix: &str, keep: fn(char) -> bool, steps: &mut usize) -> Result<Vec<String>, PasteError> {
    let mut out: Vec<String> = Vec::new();
    let mut at = 0;
    while let Some(found) = text[at..].find(prefix) {
        let start = at + found + prefix.len();
        let rest = &text[start..];
        let end = rest
            .find(|c: char| {
                *steps += 1;
                !keep(c)
            })
            .unwrap_or(rest.len());
        *steps += found + prefix.len();
        at = start + end;
        if !(8..=VALUE_MAX).contains(&end) {
            continue;
        }
        let value = format!("{prefix}{}", &rest[..end]);
        if !out.contains(&value) {
            if out.len() == CANDIDATES_MAX {
                return Err(PasteError::Ambiguous);
            }
            out.push(value);
        }
    }
    Ok(out)
}

/// A `d` cookie value as browsers send it: percent-encoded (a copy that decoded it is encoded
/// again).
fn wire(value: &str) -> String {
    value.replace('/', "%2F").replace('+', "%2B").replace('=', "%3D")
}

/// The `d-s` cookie's value: `d-s=<value>` in a cookie header, or PowerShell's
/// `Cookie("d-s", "<value>"`.
fn cookie_ds(text: &str) -> Option<String> {
    let header = text.match_indices("d-s=").find_map(|(i, _)| {
        let before = text[..i].chars().next_back();
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return None;
        }
        Some(&text[i + "d-s=".len()..])
    });
    let value = header.or_else(|| {
        let i = text.find("Cookie(\"d-s\"")?;
        let rest = &text[i + "Cookie(\"d-s\"".len()..];
        rest.trim_start_matches([',', ' ']).strip_prefix('"')
    })?;
    let end = value.find(|c: char| !is_cookie_char(c)).unwrap_or(value.len());
    (end > 0).then(|| wire(&value[..end]))
}

#[cfg(test)]
mod tests;
