//! The app's frame: what the layout of [`crate::screen`] depends on now, and the layout itself
//! with the composer views it measured, so drawing does not wrap a composer's text twice.

use super::composer::View;
use super::{App, Focus};
use crate::screen::{self, FrameLayout};
use slakio_core::layout::PaneId;

impl App {
    /// What the layout depends on now besides the size.
    fn shape(&self) -> screen::Shape {
        screen::Shape {
            rail_expanded: self.shell.rail_expanded(self.focus() == Focus::Rail),
            push: self.settings.rail_push,
            list_hidden: self.shell.list_hidden,
            thread: self.work.has_panel(),
            list_focused: self.focus() == Focus::List,
        }
    }

    /// The screen's areas now.
    pub fn areas(&self) -> screen::Areas {
        screen::areas(self.size, self.shape())
    }

    /// The whole frame now: the areas and the panes with their parts, as drawn and clicked.
    pub fn frame(&self) -> FrameLayout {
        self.frame_with_views().0
    }

    /// The frame, and the view of the composer of each pane laid out (wrapped at its width).
    /// The panes go where the work area's layout puts them; on a screen too narrow for two, the
    /// active pane is the one shown.
    pub(crate) fn frame_with_views(&self) -> (FrameLayout, Vec<(PaneId, View)>) {
        let mut views = Vec::new();
        let (tree, active) = (self.work.layout(), self.work.active());
        let layout = screen::frame(self.size, self.shape(), tree, active, |id, width| {
            let Some(pane) = self.work.pane(id) else { return 1 };
            let view = self.work.draft(pane).view(width);
            let lines = view.lines.len();
            views.push((id, view));
            lines
        });
        (layout, views)
    }
}
