//! Text on a chip (an avatar's initials, a group's count) stays readable whatever tint the theme
//! mixes the chip from: a tint near the hue itself (`avatar_tint` close to 100) would otherwise
//! put the hue on nearly the hue. Measured by WCAG 2 contrast where both colours are RGB; a chip
//! drawn on the terminal's own colours has nothing to measure and keeps them.

use super::code::{contrast, mix};
use ratatui::style::{Color, Style};

/// What initials keep at least on their chip: WCAG 2 AA for body text. They are bold but one
/// cell tall, not the "large text" that 3:1 is for.
pub const INITIALS: f64 = 4.5;

/// What a muted conversation's initials keep at least: 3:1, as muted text everywhere else.
pub const MUTED: f64 = 3.0;

const BLACK: Color = Color::Rgb(0, 0, 0);
const WHITE: Color = Color::Rgb(255, 255, 255);

/// `fg`, or the least step of it toward black or white (whichever stands further from `bg`)
/// that is `min` from `bg`. A terminal colour as `fg` on an RGB `bg` cannot be measured, so it
/// takes that pole. On a `bg` that is not RGB, `fg` as it is. Black or white is always at least
/// 4.58:1 from one or the other, so up to that every `min` is met.
pub fn readable(fg: Color, bg: Color, min: f64) -> Color {
    let (Some(black), Some(white)) = (contrast(BLACK, bg), contrast(WHITE, bg)) else { return fg };
    let pole = if black >= white { BLACK } else { WHITE };
    let start = if matches!(fg, Color::Rgb(..)) { fg } else { pole };
    (0..=20)
        .filter_map(|step| mix(pole, start, step * 5))
        .find(|c| contrast(*c, bg).is_some_and(|k| k >= min))
        .unwrap_or(pole)
}

/// `style` with its text [`readable`] on its background (unchanged without either).
pub fn on_chip(style: Style, min: f64) -> Style {
    match (style.fg, style.bg) {
        (Some(fg), Some(bg)) => style.fg(readable(fg, bg, min)),
        _ => style,
    }
}

#[cfg(test)]
mod tests;
