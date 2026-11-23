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

/// `name` is one a shortcode may have: letters, digits and `_ + - '`, at most 64 bytes.
pub fn shortcode(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || "_+-'".contains(c))
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
