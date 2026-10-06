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
//! A person's picture fills a [`Slot`] beside their message: 4 × 2 cells in the comfortable
//! layout (about square in a terminal's cells), 2 × 1 where a pane is narrow. What fills it is
//! an [`Art`]: initials now, drawn on the slot's first row with the rest of the block in the same
//! tint; a profile photo (a later step, the kitty graphics protocol) fills the same cells, and
//! initials stay its fallback where a terminal cannot show images.

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

/// The cells a person's picture takes beside their message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub cols: usize,
    pub rows: usize,
}

/// The comfortable layout's slot: 4 × 2 cells.
pub const BLOCK: Slot = Slot { cols: 4, rows: 2 };
/// A narrow pane's slot: the 2-cell chip on one row.
pub const CHIP: Slot = Slot { cols: WIDTH, rows: 1 };

/// What fills a slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Art {
    /// The person's initials ([`initials`], [`WIDTH`] cells), centered on the slot's first row.
    Initials(String),
}

impl Art {
    /// The text of row `k` of `slot` (exactly `slot.cols` cells; blank past the art).
    pub fn row(&self, slot: Slot, k: usize) -> String {
        match self {
            Art::Initials(i) if k == 0 => {
                let pad = slot.cols.saturating_sub(WIDTH);
                format!("{}{i}{}", " ".repeat(pad / 2), " ".repeat(pad - pad / 2))
            }
            Art::Initials(_) => " ".repeat(slot.cols),
        }
    }
}

#[cfg(test)]
mod tests;
