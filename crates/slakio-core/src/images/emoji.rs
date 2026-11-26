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
//! Only names a shortcode may have are read ([`shortcode`]), at most [`NAMES_MAX`] of them; a
//! name (or an alias's target) written in decomposed Hangul jamo is read as its syllables
//! ([`compose_hangul`]), and where both forms of one name are listed the syllables' one wins.

use super::emoji_address;
use std::borrow::Cow;
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;

mod hangul;
pub use hangul::compose_hangul;

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

/// The format characters (Unicode's `Cf`: joiners, bidi controls, invisible operators, tags),
/// the private-use planes, the conjoining Hangul jamo and fillers, and the lookalikes of ASCII
/// letters and digits: refused, though some are letters.
const REFUSED: &[(char, char)] = &[
    ('\u{00AD}', '\u{00AD}'),
    // Hangul jamo that conjoin into a syllable, and the Hangul fillers: a name built of them
    // looks like one of precomposed syllables (U+AC00 to U+D7A3), which stay letters.
    ('\u{1100}', '\u{11FF}'),
    ('\u{3164}', '\u{3164}'),
    ('\u{A960}', '\u{A97F}'),
    ('\u{D7B0}', '\u{D7FF}'),
    // Letters and digits that look like ASCII ones: fullwidth forms (digits too), the halfwidth
    // Hangul and symbols, enclosed and mathematical alphanumerics. The halfwidth katakana
    // letters and prolonged sound mark (U+FF66 to U+FF9D) are real letters and left out here;
    // the halfwidth middle dot (U+FF65, punctuation, not a letter) and voiced marks (U+FF9E and
    // U+FF9F, which join the letter before) are left out too: the other rules refuse them.
    ('\u{2460}', '\u{24FF}'),
    ('\u{FF00}', '\u{FF64}'),
    ('\u{FFA0}', '\u{FFEF}'),
    ('\u{1D400}', '\u{1D7FF}'),
    ('\u{1F130}', '\u{1F189}'),
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

/// The decimal digits of every script (Unicode 15.0's `Nd`): the fullwidth ones
/// and the mathematical ones are refused all the same ([`REFUSED`]).
const DIGITS: &[(char, char)] = &[
    ('\u{0030}', '\u{0039}'),
    ('\u{0660}', '\u{0669}'),
    ('\u{06F0}', '\u{06F9}'),
    ('\u{07C0}', '\u{07C9}'),
    ('\u{0966}', '\u{096F}'),
    ('\u{09E6}', '\u{09EF}'),
    ('\u{0A66}', '\u{0A6F}'),
    ('\u{0AE6}', '\u{0AEF}'),
    ('\u{0B66}', '\u{0B6F}'),
    ('\u{0BE6}', '\u{0BEF}'),
    ('\u{0C66}', '\u{0C6F}'),
    ('\u{0CE6}', '\u{0CEF}'),
    ('\u{0D66}', '\u{0D6F}'),
    ('\u{0DE6}', '\u{0DEF}'),
    ('\u{0E50}', '\u{0E59}'),
    ('\u{0ED0}', '\u{0ED9}'),
    ('\u{0F20}', '\u{0F29}'),
    ('\u{1040}', '\u{1049}'),
    ('\u{1090}', '\u{1099}'),
    ('\u{17E0}', '\u{17E9}'),
    ('\u{1810}', '\u{1819}'),
    ('\u{1946}', '\u{194F}'),
    ('\u{19D0}', '\u{19D9}'),
    ('\u{1A80}', '\u{1A89}'),
    ('\u{1A90}', '\u{1A99}'),
    ('\u{1B50}', '\u{1B59}'),
    ('\u{1BB0}', '\u{1BB9}'),
    ('\u{1C40}', '\u{1C49}'),
    ('\u{1C50}', '\u{1C59}'),
    ('\u{A620}', '\u{A629}'),
    ('\u{A8D0}', '\u{A8D9}'),
    ('\u{A900}', '\u{A909}'),
    ('\u{A9D0}', '\u{A9D9}'),
    ('\u{A9F0}', '\u{A9F9}'),
    ('\u{AA50}', '\u{AA59}'),
    ('\u{ABF0}', '\u{ABF9}'),
    ('\u{FF10}', '\u{FF19}'),
    ('\u{104A0}', '\u{104A9}'),
    ('\u{10D30}', '\u{10D39}'),
    ('\u{11066}', '\u{1106F}'),
    ('\u{110F0}', '\u{110F9}'),
    ('\u{11136}', '\u{1113F}'),
    ('\u{111D0}', '\u{111D9}'),
    ('\u{112F0}', '\u{112F9}'),
    ('\u{11450}', '\u{11459}'),
    ('\u{114D0}', '\u{114D9}'),
    ('\u{11650}', '\u{11659}'),
    ('\u{116C0}', '\u{116C9}'),
    ('\u{11730}', '\u{11739}'),
    ('\u{118E0}', '\u{118E9}'),
    ('\u{11950}', '\u{11959}'),
    ('\u{11C50}', '\u{11C59}'),
    ('\u{11D50}', '\u{11D59}'),
    ('\u{11DA0}', '\u{11DA9}'),
    ('\u{11F50}', '\u{11F59}'),
    ('\u{16A60}', '\u{16A69}'),
    ('\u{16AC0}', '\u{16AC9}'),
    ('\u{16B50}', '\u{16B59}'),
    ('\u{1D7CE}', '\u{1D7FF}'),
    ('\u{1E140}', '\u{1E149}'),
    ('\u{1E2F0}', '\u{1E2F9}'),
    ('\u{1E4F0}', '\u{1E4F9}'),
    ('\u{1E950}', '\u{1E959}'),
    ('\u{1FBF0}', '\u{1FBF9}'),
];

/// `name` is one a shortcode may have, as Slack names emoji: letters of any script (a Korean
/// name of precomposed syllables too), decimal digits of any script and `_ + - '`, at most [`SHORTCODE_CHARS`] characters and
/// [`SHORTCODE_BYTES`] bytes. Never a control, format or private-use character, a blank, a `:`,
/// or a mark that joins the character before it (an accent, a vowel sign, a variation selector):
/// a precomposed Hangul syllable is one letter, and a name of stacked marks draws nothing.
pub fn shortcode(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= SHORTCODE_BYTES
        && name.chars().count() <= SHORTCODE_CHARS
        && name.chars().all(|c| matches!(c, '_' | '+' | '-' | '\'') || letter(c))
}

/// `c` is a letter (of any script, not a number written as letters, as Roman numerals) or a
/// decimal digit (not a superscript, a fraction or a circled number) a shortcode may hold.
fn letter(c: char) -> bool {
    (DIGITS.iter().any(|(from, to)| (*from..=*to).contains(&c)) || (c.is_alphabetic() && !c.is_numeric()))
        && !c.is_control()
        && !c.is_whitespace()
        && !REFUSED.iter().any(|(from, to)| (*from..=*to).contains(&c))
        && !joins(c)
}

/// `c` is a mark that joins the character before it: after `a`, the two stay one cluster.
fn joins(c: char) -> bool {
    let mut buf = [0u8; 5];
    buf[0] = b'a';
    let n = 1 + c.encode_utf8(&mut buf[1..]).len();
    !std::str::from_utf8(&buf[..n]).is_ok_and(|s| s.graphemes(true).count() == 2)
}

/// What each name of `list` (`emoji.list`'s `name → address or alias:other`) draws, by name; a
/// name that draws nothing of its own is left out.
pub fn resolve(list: &[(String, String)]) -> Vec<(String, Custom)> {
    // Each name with its value and whether it was written in jamo (`true`), which never
    // replaces the same name written in syllables.
    let mut kept: HashMap<String, (String, bool)> = HashMap::new();
    let named = list.iter().map(|(name, value)| (compose_hangul(name), value)).filter(|(name, _)| shortcode(name));
    for (name, value) in named.take(NAMES_MAX) {
        let value = match value.strip_prefix("alias:") {
            Some(next) => format!("alias:{}", compose_hangul(next)),
            None => value.clone(),
        };
        let jamo = matches!(name, Cow::Owned(_));
        if !jamo || kept.get(name.as_ref()).is_none_or(|(_, was)| *was) {
            kept.insert(name.into_owned(), (value, jamo));
        }
    }
    let names: HashMap<&str, &str> = kept.iter().map(|(name, (value, _))| (name.as_str(), value.as_str())).collect();
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
