//! A message's reactions as chips, as Slack's desktop app draws them: quiet pills a little apart
//! from the background, the emoji then the count, one cell apart, wrapped at the pane's width.
//! The user's own sits on a light wash of the accent, its count in the accent
//! ([`crate::theme::Theme::reaction_chip`]).
//!
//! ```text
//!  icons on:   (:+1: 3) (:eyes: 1)     each end a rounded cap in the chip's colour
//!  icons off:  _:+1: 3_ _:eyes: 1_     a blank pad at each end, on the chip's colour
//! ```
//!
//! A cap takes the pad's one cell, so a chip is as wide either way: where it wraps, where its
//! first cell sits (in the gap before the text) and where a click lands are the same.

use crate::app::App;
use crate::app::pane::Shown;
use crate::emoji;
use crate::text::{clip, width};
use ratatui::style::Style;
use ratatui::text::Span;
use slakio_core::model::WorkspaceId;

/// The left end of a pill (a Nerd Font glyph, one cell).
pub const CAP_LEFT: &str = "\u{E0B6}";
/// The right end of a pill (a Nerd Font glyph, one cell).
pub const CAP_RIGHT: &str = "\u{E0B4}";

/// The reactions of `m` (of workspace `ws`) as chips: a cap (icons on) or a pad, the emoji, the
/// count, a cap or a pad; one cell apart, wrapped at `w` cells. An emoji Slack names that is not
/// a standard one is the workspace's own picture for it where it can be drawn
/// ([`super::custom`]; its cells keep their colour, the picture's id), else `:name:`.
pub fn chips(app: &App, ws: &WorkspaceId, m: &Shown, w: usize) -> Vec<Vec<Span<'static>>> {
    let t = &app.theme;
    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    let mut used = 0;
    for r in &m.reactions {
        let name = r.name.as_str();
        let look = t.reaction_chip(r.mine);
        let ends: [(&str, Style); 2] = match look.cap.filter(|_| app.settings.icons) {
            Some(cap) => [(CAP_LEFT, cap), (CAP_RIGHT, cap)],
            None => [(" ", look.body), (" ", look.body)],
        };
        let mut runs = match emoji::get(name) {
            Some(face) => vec![(face.to_string(), look.body)],
            None => super::custom::runs(app, ws, &format!(":{name}:"), look.body, &m.emoji_pictures),
        };
        runs.insert(0, (ends[0].0.to_string(), ends[0].1));
        runs.push((format!(" {}", r.count), look.count));
        runs.push((ends[1].0.to_string(), ends[1].1));
        let mut cw: usize = runs.iter().map(|(s, _)| width(s)).sum();
        // A chip wider than the pane is its text, cut.
        if cw > w.max(1) {
            let face = emoji::get(name).map_or_else(|| format!(":{name}:"), str::to_string);
            let text = format!(" {face} {} ", r.count);
            (runs, cw) = (vec![(clip(&text, w.max(1)), look.body)], width(&text));
        }
        if lines.is_empty() || (used > 0 && used + 1 + cw > w) {
            lines.push(Vec::new());
            used = 0;
        }
        let line = lines.last_mut().expect("a line was pushed");
        if used > 0 {
            line.push(Span::raw(" "));
            used += 1;
        }
        line.extend(runs.into_iter().map(|(s, st)| Span::styled(s, st)));
        used += cw;
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{CAP_LEFT, CAP_RIGHT};
    use crate::text::width;

    /// A cap takes exactly the pad's cell, so a chip is as wide with icons as without.
    #[test]
    fn a_cap_is_one_cell_like_the_pad_it_replaces() {
        for cap in [CAP_LEFT, CAP_RIGHT] {
            assert_eq!(width(cap), width(" "), "{cap:?}");
        }
    }
}
