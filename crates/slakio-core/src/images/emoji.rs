//! A workspace's own emoji as Slack's `emoji.list` names them: each name the address of its
//! picture, or `alias:<other>`, another name for an emoji (one of the workspace's, or a standard
//! one). [`resolve`] reads such a list into what each name draws ([`Custom`]):
//!
//! * a name whose picture is on the emoji host ([`super::emoji_address`]) draws that picture; one
//!   anywhere else (another host, `http`, a port, a user) draws nothing of its own (`:name:`);
//! * an alias is followed, at most [`ALIAS_DEPTH`] times; one that comes back to a name it went
//!   through (a cycle), goes deeper, or names what no shortcode is, draws nothing of its own;
//! * an alias that ends at a name the workspace has not (a standard emoji's, as `:+1:`) draws
//!   that standard emoji, where the UI knows it.
//!
//! Only names a shortcode may have are read ([`shortcode`]), at most [`NAMES_MAX`] of them.

use super::emoji_address;
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;

/// The most aliases followed from one name.
pub const ALIAS_DEPTH: usize = 4;

/// The most names of one workspace read.
pub const NAMES_MAX: usize = 65_536;

/// What a workspace's own emoji draws.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Custom {
    /// Its picture, at this address on the emoji host.
    Picture(String),
    /// The standard emoji of this name (an alias of one).
    Standard(String),
}

/// The most characters a shortcode has.
pub const SHORTCODE_CHARS: usize = 64;

/// The most bytes a shortcode has: 64 characters of a Latin name, about 33 of a Korean one.
pub const SHORTCODE_BYTES: usize = 100;

/// The format characters (Unicode's `Cf`: joiners, bidi controls, invisible operators, tags) and
/// the private-use planes: named here so they stay refused, though no letter or digit is one.
const REFUSED: &[(char, char)] = &[
    ('\u{00AD}', '\u{00AD}'),
    ('\u{0600}', '\u{0605}'),
    ('\u{061C}', '\u{061C}'),
    ('\u{06DD}', '\u{06DD}'),
    ('\u{070F}', '\u{070F}'),
    ('\u{0890}', '\u{0891}'),
    ('\u{08E2}', '\u{08E2}'),
    ('\u{180E}', '\u{180E}'),
    ('\u{200B}', '\u{200F}'),
    ('\u{202A}', '\u{202E}'),
    ('\u{2060}', '\u{2064}'),
    ('\u{2066}', '\u{206F}'),
    ('\u{E000}', '\u{F8FF}'),
    ('\u{FEFF}', '\u{FEFF}'),
    ('\u{FFF9}', '\u{FFFB}'),
    ('\u{110BD}', '\u{110BD}'),
    ('\u{110CD}', '\u{110CD}'),
    ('\u{13430}', '\u{1343F}'),
    ('\u{1BCA0}', '\u{1BCA3}'),
    ('\u{1D173}', '\u{1D17A}'),
    ('\u{E0001}', '\u{E007F}'),
    ('\u{F0000}', '\u{10FFFF}'),
];

/// `name` is one a shortcode may have, as Slack names emoji: letters and digits of any script
/// (a Korean name too) and `_ + - '`, at most [`SHORTCODE_CHARS`] characters and
/// [`SHORTCODE_BYTES`] bytes. Never a control, format or private-use character, a blank, a `:`,
/// or a mark that joins the character before it (an accent, a vowel sign, a variation selector):
/// a precomposed Hangul syllable is one letter, and a name of stacked marks draws nothing.
pub fn shortcode(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= SHORTCODE_BYTES
        && name.chars().count() <= SHORTCODE_CHARS
        && name.chars().all(|c| matches!(c, '_' | '+' | '-' | '\'') || letter(c))
}

/// `c` is a letter or a digit a shortcode may hold.
fn letter(c: char) -> bool {
    c.is_alphanumeric()
        && !c.is_control()
        && !c.is_whitespace()
        && !REFUSED.iter().any(|(from, to)| (*from..=*to).contains(&c))
        // A mark joins the character before it: after `a`, the two stay one cluster.
        && ['a', c].iter().collect::<String>().graphemes(true).count() == 2
}

/// What each name of `list` (`emoji.list`'s `name → address or alias:other`) draws, by name; a
/// name that draws nothing of its own is left out.
pub fn resolve(list: &[(String, String)]) -> Vec<(String, Custom)> {
    let names: HashMap<&str, &str> = (list.iter())
        .filter(|(name, _)| shortcode(name))
        .take(NAMES_MAX)
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let mut out: Vec<(String, Custom)> =
        names.keys().filter_map(|name| follow(&names, name).map(|c| (name.to_string(), c))).collect();
    out.sort();
    out
}

/// What `name` (one of `names`) draws, its aliases followed.
fn follow(names: &HashMap<&str, &str>, name: &str) -> Option<Custom> {
    let mut seen = vec![name];
    let mut at = name;
    loop {
        let Some(value) = names.get(at) else { return Some(Custom::Standard(at.to_string())) };
        let Some(next) = value.strip_prefix("alias:") else { return emoji_address(value).map(Custom::Picture) };
        if seen.len() > ALIAS_DEPTH || seen.contains(&next) || !shortcode(next) {
            return None;
        }
        seen.push(next);
        at = next;
    }
}

#[cfg(test)]
mod tests;
