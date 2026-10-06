//! The popups over the screen, in one order. The top one takes the keys
//! ([`App::key_context`]) and the mouse, and drawing paints them bottom up, so what is drawn on
//! top is always what the keys and the mouse go to.
//!
//! From the top: a question ([`super::dialog`]), the keyboard help, the command palette, the
//! workspace switcher ([`super::nav`]), the which-key popup. The palette and the which-key popup are drawn only on top; the help stays
//! visible under a question.

use super::App;
use super::query::Overlay;
use std::time::Instant;

/// A popup over the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Dialog,
    Help,
    Palette,
    Switcher,
    WhichKey,
}

impl App {
    /// The popups up at `now`, the top one first (without `now`, the which-key popup, which
    /// takes no keys of its own, is left out).
    pub(crate) fn layers(&self, now: Option<Instant>) -> Vec<Layer> {
        let up = [
            (Layer::Dialog, self.dialog.is_some()),
            (Layer::Help, self.help.is_some()),
            (Layer::Palette, self.cmdline.is_open()),
            (Layer::Switcher, self.switcher.is_some()),
            (Layer::WhichKey, now.is_some_and(|t| self.which_key_visible(t))),
        ];
        up.into_iter().filter(|(_, on)| *on).map(|(l, _)| l).collect()
    }

    /// The popup that takes the keys and the mouse, if one is up.
    pub fn overlay(&self) -> Option<Overlay> {
        match self.layers(None).first()? {
            Layer::Dialog => self.dialog.map(|d| Overlay::Dialog(d.question)),
            Layer::Help => Some(Overlay::Help),
            Layer::Palette => Some(Overlay::Palette),
            Layer::Switcher => Some(Overlay::Switcher),
            Layer::WhichKey => None,
        }
    }
}
