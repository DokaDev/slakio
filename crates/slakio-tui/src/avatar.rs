//! Initials avatars: a person pictured as a two-cell chip of their initials on a color of their
//! own, as GUI chat apps do before (or instead of) a photo.
//!
//! * Initials come from the name people see, already sanitised: the first letters of the first
//!   and the last word (`Minsu Kim` → `MK`), one letter for a single word (`Hana` → `H`), and
//!   for a name that starts with a wide character, Hangul or other CJK, that first syllable
//!   alone (Korean names start with the family name). Only letters and digits count, so marks,
//!   emoji and what the sanitiser left of control characters never reach the chip; a name
//!   without any falls back to the handle, then to `?`.
//! * The color is a slot of the theme's avatar colors ([`crate::theme::Theme::avatars`]) picked
//!   by a hash of the person's id: the same in every run and every theme, and kept apart from
//!   the workspace colors.
//!
//! Profile photos come later; where a terminal cannot show them, these chips stay.

use crate::text::width;
use slakio_core::model::UserId;

/// The cells a chip takes.
pub const WIDTH: usize = 2;

/// The initials of a person named `name`, exactly [`WIDTH`] cells wide: two narrow letters,
/// one narrow letter and a space, or one wide character. `handle` gives the handle, asked for
/// only when the name has no letter or digit.
pub fn initials(name: &str, handle: impl FnOnce() -> String) -> String {
    from_words(name).or_else(|| from_words(&handle())).unwrap_or_else(|| "? ".to_string())
}

/// The first letter or digit of a word, upper case, when it is drawn one or two cells wide.
fn first(word: &str) -> Option<char> {
    let c = word.chars().find(|c| c.is_alphanumeric())?;
    let up = c.to_uppercase().next().unwrap_or(c);
    matches!(width(up.encode_utf8(&mut [0; 4])), 1 | 2).then_some(up)
}

fn from_words(name: &str) -> Option<String> {
    let mut words = name.split(|c: char| c.is_whitespace() || matches!(c, '.' | '_' | '-')).filter_map(first);
    let a = words.next()?;
    let wide = |c: char| width(c.encode_utf8(&mut [0; 4])) == 2;
    if wide(a) {
        return Some(a.to_string());
    }
    match words.next_back().filter(|&b| !wide(b)) {
        Some(b) => Some(format!("{a}{b}")),
        None => Some(format!("{a} ")),
    }
}

/// The color slot of the person `id`: FNV-1a of the id, stable across runs and versions.
pub fn slot(id: &UserId) -> usize {
    let mut h: u32 = 0x811C_9DC5;
    for b in id.as_str().bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h as usize
}

#[cfg(test)]
mod tests;
