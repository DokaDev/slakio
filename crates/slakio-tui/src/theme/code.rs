//! The tint code is drawn on (inline code, a code block's lines): far enough from the row it is
//! on to be seen on any display, on the background and on a selected row alike, with body text
//! on it still readable.

use super::bars::luminance;
use super::{Kind, Theme};
use ratatui::style::Color;

/// The contrast code's tint keeps at least from the row's background.
pub const CODE_APART: f64 = 1.3;

/// What body text keeps at least on code's tint.
const TEXT_ON_CODE: f64 = 4.5;

/// The contrast of two RGB colors (`None` for a terminal color).
fn contrast(a: Color, b: Color) -> Option<f64> {
    let (x, y) = (luminance(a)?, luminance(b)?);
    Some((x.max(y) + 0.05) / (x.min(y) + 0.05))
}

/// `percent` of `c` mixed into `base` (`None`: not RGB).
fn mix(c: Color, base: Color, percent: u16) -> Option<Color> {
    let (Color::Rgb(r, g, b), Color::Rgb(br, bg, bb)) = (c, base) else { return None };
    let m = |a: u8, z: u8| ((u16::from(a) * percent + u16::from(z) * (100 - percent)) / 100) as u8;
    Some(Color::Rgb(m(r, br), m(g, bg), m(b, bb)))
}

impl Theme {
    /// The background of code on the app's background.
    pub fn code_bg(&self) -> Color {
        self.code_on(self.bg)
    }

    /// The background of code on a row of background `row` (the app's, or a selection bar's):
    /// the least mix of body text into it, else of the app's background, that sets it
    /// [`CODE_APART`] from the row with body text still 4.5:1 on it (where none does, as far
    /// from the row as body text allows: a light bar close to the background). Without RGB colors, the
    /// status line's surface (or the raised one where that is the background).
    pub fn code_on(&self, row: Color) -> Color {
        let fallback = if self.surface == self.bg { self.raised() } else { self.surface };
        if self.kind != Kind::Truecolor {
            return fallback;
        }
        let readable: Vec<(Color, f64)> = [self.fg, self.bg]
            .into_iter()
            .flat_map(|toward| (1..=50).filter_map(move |step| mix(toward, row, step * 2)))
            .filter(|c| contrast(self.fg, *c).is_some_and(|k| k >= TEXT_ON_CODE))
            .filter_map(|c| Some((c, contrast(c, row)?)))
            .collect();
        let best = readable.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1));
        readable.iter().find(|(_, k)| *k >= CODE_APART).or(best.as_ref()).map_or(fallback, |(c, _)| *c)
    }
}
