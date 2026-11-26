//! The name of a conversation in the list: unread bold in body text, read muted. A muted
//! conversation is faint whatever it holds ([`Theme::faint`]); the list asks for that first.

use super::Theme;
use super::code::{contrast, mix};
use ratatui::style::{Color, Style};

/// The least contrast of an unread name from a read one.
pub const READ_APART: f64 = 1.5;

/// The least contrast of a read name on the background.
pub const READ_ON_BG: f64 = 3.5;

/// The least contrast of a read name on the hover colour.
pub const READ_ON_HOVER: f64 = 3.0;

impl Theme {
    /// A conversation's name in the list: bold body text when `unread`, else muted. Without
    /// colour the weight alone tells them apart.
    pub fn row_name(&self, unread: bool) -> Style {
        if unread { self.bold() } else { self.muted() }
    }

    /// `c` reads as a read name: on the background and the hover colour, and apart from body
    /// text. A colour that is not RGB passes (nothing to measure).
    fn reads_muted(&self, c: Color) -> bool {
        let at_least = |a: Color, b: Color, k: f64| contrast(a, b).is_none_or(|x| x >= k);
        at_least(c, self.bg, READ_ON_BG) && at_least(c, self.hover, READ_ON_HOVER) && at_least(c, self.fg, READ_APART)
    }

    /// The muted colour a theme file leaves out, where it sets `bg` or `fg` and the base's muted
    /// colour no longer reads as a read name ([`Self::reads_muted`]): the most of body text
    /// mixed into the background (from 85 percent down) that does, else 65 percent. A colour
    /// that is not RGB keeps the base's.
    pub(super) fn derive_muted(&mut self, set: impl Fn(&str) -> bool) {
        if set("fg_muted") || !(set("bg") || set("fg")) || self.reads_muted(self.fg_muted) {
            return;
        }
        let found = (30..=85).rev().filter_map(|p| mix(self.fg, self.bg, p)).find(|c| self.reads_muted(*c));
        if let Some(c) = found.or_else(|| mix(self.fg, self.bg, 65)) {
            self.fg_muted = c;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{READ_APART, READ_ON_BG, READ_ON_HOVER};
    use crate::theme::code::contrast;
    use crate::theme::{BUILTINS, Kind, Theme};
    use ratatui::style::Modifier;

    #[test]
    fn read_and_unread_rows_differ_in_every_builtin() {
        let all = BUILTINS.iter().map(|t| (*t).clone()).chain([Theme::terminal(), Theme::no_color()]);
        for t in all {
            let n = &t.name;
            let (unread, read) = (t.row_name(true), t.row_name(false));
            assert!(unread.add_modifier.contains(Modifier::BOLD), "{n}: unread is bold");
            assert!(!read.add_modifier.contains(Modifier::BOLD), "{n}: read is not");
            assert_ne!(unread, read, "{n}");
            if t.kind != Kind::Truecolor {
                continue;
            }
            let (u, r) = (unread.fg.unwrap(), read.fg.unwrap());
            let k = contrast(u, r).unwrap();
            assert!(k >= READ_APART, "{n}: unread from read {k:.2}");
            let k = contrast(r, t.bg).unwrap();
            assert!(k >= READ_ON_BG, "{n}: read on the background {k:.2}");
            let k = contrast(r, t.hover).unwrap();
            assert!(k >= READ_ON_HOVER, "{n}: read on the hover colour {k:.2}");
        }
    }
}
