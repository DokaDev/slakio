//! The app's frame: what the layout of [`crate::screen`] depends on now, and the layout itself
//! with the composer views it measured, so drawing does not wrap a composer's text twice.

use super::composer::View;
use super::{App, Focus};
use crate::screen::{self, FrameLayout};
use slakio_core::layout::PaneId;

impl App {
    /// What the layout depends on now besides the size. The list panel makes room for a thread
    /// panel only where one is laid out in that room (not one left out for room, or zoomed out).
    fn shape(&self) -> screen::Shape {
        let mut s = screen::Shape {
            list_hidden: self.shell.list_hidden,
            thread: self.work.has_panel() && !self.work.zoomed(),
            list_focused: matches!(self.focus(), Focus::List | Focus::ViewSwitcher),
            tabs: self.work.tabs().len() > 1,
            wide_list: self.shell.view == super::shell::View::Activity,
        };
        if s.thread {
            let work = screen::areas(self.size, s).work;
            let laid = self.work.layout().map(|t| screen::work_panes(work, t, self.work.active())).unwrap_or_default();
            s.thread = laid.iter().any(|(id, _)| self.work.owner(*id).is_some());
        }
        s
    }

    /// The work area with no thread panel taking the list panel's room: a split measures its
    /// room there, since it may take the panel out of the layout and the list come back.
    pub(super) fn plain_work(&self) -> ratatui::layout::Rect {
        screen::areas(self.size, screen::Shape { thread: false, ..self.shape() }).work
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
    /// The panes go where the work area's layout puts them; on a screen too narrow for two, and
    /// in a zoomed tab, the active pane is the one shown.
    pub(crate) fn frame_with_views(&self) -> (FrameLayout, Vec<(PaneId, View)>) {
        let mut views = Vec::new();
        let (tree, active) = (self.work.shown_layout(), self.work.active());
        let layout = screen::frame(self.size, self.shape(), tree.as_deref(), active, |id, width| {
            let Some(pane) = self.work.pane(id) else { return 1 };
            let view = self.work.draft(pane).view(width);
            let lines = view.lines.len();
            views.push((id, view));
            lines
        });
        (layout, views)
    }
}
