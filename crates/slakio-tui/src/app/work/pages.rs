//! Pages of messages as they arrive for the panes ([`super::Work`]), and the ones that failed:
//! taken by the timeline that waits for them, said where the active pane shows them, asked for
//! again on `r`.

use super::{Work, shown};
use crate::app::model::Model;
use slakio_core::backend::{FailKind, Generation, Page};
use slakio_core::model::WorkspaceId;
use std::time::Instant;

impl Work {
    /// A page of messages arrived. `true` when a timeline took it; an answer to an older
    /// request or for a target no pane shows is dropped. Every pane on the target keeps its
    /// selection on its messages (it names them by timestamp).
    pub fn on_page(&mut self, generation: Generation, page: &Page, model: &Model) -> bool {
        if self.on_caught_up(generation, page, model) {
            return true;
        }
        let Some(tl) = self.timelines.awaiting(&page.target, generation) else { return false };
        let shown = page.messages.iter().map(|m| shown(model, &page.target, m)).collect();
        tl.add_page(page, shown);
        self.fill();
        true
    }

    /// The local cache's newest page for a target whose page `generation` is awaited: drawn
    /// until that page comes. `false` when nothing awaits it, or messages are shown already.
    pub fn on_cached_page(&mut self, generation: Generation, page: &Page, model: &Model) -> bool {
        let Some(tl) = self.timelines.awaiting(&page.target, generation) else { return false };
        if !tl.items.is_empty() {
            return false;
        }
        tl.add_cached(page.messages.iter().map(|m| shown(model, &page.target, m)).collect());
        true
    }

    /// The active pane shows the local cache's messages, not (yet) the backend's.
    pub fn active_cached(&self) -> bool {
        self.active().and_then(|id| self.panes.get(&id)).is_some_and(|p| self.timeline(p).cached)
    }

    /// Request `id` for a page failed at `now`: its timeline stops waiting and asks no more
    /// until retried (by `r`, or by itself when Slack asked to slow down). The workspace of the
    /// page; `None` when nothing waited for it.
    pub fn on_failed(&mut self, id: Generation, kind: FailKind, now: Instant) -> Option<WorkspaceId> {
        let target = self.timelines.fail(id, kind, now)?;
        self.forget_jumps(&target);
        Some(target.workspace().clone())
    }

    /// Why the active pane's messages could not be loaded, if they could not.
    pub fn active_failed(&self) -> Option<&FailKind> {
        let pane = self.active().and_then(|id| self.panes.get(&id))?;
        self.timeline(pane).failed.as_ref()
    }

    /// The active pane's failed page may be asked for again now ([`Timeline::retryable`]).
    pub fn active_retryable(&self) -> bool {
        self.active().and_then(|id| self.panes.get(&id)).is_some_and(|p| self.timeline(p).retryable())
    }

    /// The active pane's messages (its newest page, or an older one) could not be loaded, for a
    /// reason that may pass: ask again. `false` when there is nothing to retry there.
    pub fn retry(&mut self) -> bool {
        let Some(target) = self.active().and_then(|id| self.panes.get(&id)).map(|p| p.target.clone()) else {
            return false;
        };
        let tl = self.timelines.entry(&target);
        if !tl.retryable() {
            return false;
        }
        tl.queued = tl.older_failed().is_some();
        (tl.failed, tl.retry_at) = (None, None);
        self.fill();
        true
    }
}
