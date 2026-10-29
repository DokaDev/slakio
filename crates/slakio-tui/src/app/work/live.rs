//! Live updates in the panes: a message posted, changed or deleted goes into every timeline that
//! shows where it belongs — the conversation, the thread it is a reply of — and the count of
//! replies under a thread's own message follows. Panes name their selection and anchor by
//! timestamp, and one where the user chose a message keeps its view, so nothing moves under the
//! user; a pane following the newest message keeps following it. After live updates were lost a
//! while, the open conversations and threads are read again, one newest page each
//! ([`Work::catch_up`]); one that fails is asked for again by itself later.

use super::{Work, shown};
use crate::app::model::Model;
use slakio_core::backend::{Command, Generation, Page};
use slakio_core::model::{ConversationId, Message, Target, Ts, WorkspaceId};

/// The most conversations of a workspace read again after live updates were lost.
pub const CATCHUP_MAX: usize = 8;

/// The most threads of a workspace read again after live updates were lost (after its
/// conversations).
pub const CATCHUP_THREADS_MAX: usize = 4;

/// The messages each one reads again: the newest page an open shows.
pub const CATCHUP_PAGE: u32 = 28;

/// Where a live message goes.
#[derive(Clone, Copy, Debug)]
pub struct Place<'a> {
    pub workspace: &'a WorkspaceId,
    pub conversation: &'a ConversationId,
    /// The thread's own message, for a reply.
    pub thread: Option<Ts>,
    /// A reply shown in the conversation as well.
    pub broadcast: bool,
}

impl Place<'_> {
    fn conversation(&self) -> Target {
        Target::Conversation { workspace: self.workspace.clone(), conversation: self.conversation.clone() }
    }

    fn thread(&self, thread: Ts) -> Target {
        Target::Thread { workspace: self.workspace.clone(), conversation: self.conversation.clone(), thread }
    }

    /// `target` is the conversation or one of its threads.
    fn holds(&self, target: &Target) -> bool {
        target.workspace() == self.workspace && target.conversation() == self.conversation
    }
}

impl Work {
    /// Message `m` arrived live at `at` (a new copy of one when `changed`). `true` when a pane's
    /// messages changed.
    pub fn on_live_message(&mut self, at: Place<'_>, changed: bool, m: &Message, model: &Model) -> bool {
        let mut drawn = false;
        if changed {
            // Wherever it is loaded: the conversation, or a thread (its own message, or a reply).
            for (target, tl) in self.timelines.iter_mut().filter(|(t, _)| at.holds(t)) {
                drawn |= tl.put_live(shown(model, target, m), true);
            }
            return drawn;
        }
        // A pane where the user chose a message stays where it is: the new one comes in below
        // what it shows. One following the newest keeps following it.
        let into: Vec<Target> =
            [(at.thread.is_none() || at.broadcast).then(|| at.conversation()), at.thread.map(|t| at.thread(t))]
                .into_iter()
                .flatten()
                .collect();
        self.pin(&into);
        if at.thread.is_none() || at.broadcast {
            let target = at.conversation();
            if let Some(tl) = self.timelines.get_mut(&target) {
                drawn |= tl.put_live(shown(model, &target, m), false);
            }
        }
        if let Some(parent) = at.thread {
            let target = at.thread(parent);
            if let Some(tl) = self.timelines.get_mut(&target) {
                drawn |= tl.put_live(shown(model, &target, m), false);
                drawn |= tl.add_reply(parent, m.ts);
            }
            if let Some(tl) = self.timelines.get_mut(&at.conversation()) {
                drawn |= tl.add_reply(parent, m.ts);
            }
        }
        drawn
    }

    /// The panes on `targets` where the user chose a message keep their view while what they
    /// show changes: the selected message stays on its row (held there), whatever comes in
    /// below it; until a pane is drawn, its bottom is pinned to the newest message shown. What is
    /// newer than that newest one counts as new below the view.
    fn pin(&self, targets: &[Target]) {
        for pane in self.panes.values().filter(|p| targets.contains(&p.target) && p.selected.is_some()) {
            let newest = self.timelines.get(&pane.target).and_then(|tl| tl.items.last()).map(|m| m.ts);
            if pane.bottom.get().is_none() {
                pane.bottom.set(newest);
            }
            if pane.held.get().is_none() {
                pane.held.set(pane.below.get().filter(|(ts, _)| pane.selected == Some(*ts)));
            }
            if pane.fresh.get().is_none() {
                pane.fresh.set(newest);
                let items = self.timelines.get(&pane.target).map(|tl| &tl.items);
                let below = |ts: &Ts| pane.selected.is_some_and(|s| *ts > s);
                *pane.seen.borrow_mut() = items.into_iter().flatten().map(|m| m.ts).filter(below).collect();
            }
        }
    }

    /// The message at `ts` was deleted at `at` (a reply, when `at.thread` says so). `true` when a
    /// pane's messages changed. A reply comes off its thread's count only when it was loaded and
    /// taken away now: a deletion said twice, or one of a reply never shown, counts nothing.
    pub fn on_live_deleted(&mut self, at: Place<'_>, ts: Ts) -> bool {
        let mut removed = false;
        for (_, tl) in self.timelines.iter_mut().filter(|(t, _)| at.holds(t)) {
            removed |= tl.remove(ts);
        }
        let mut drawn = removed;
        if let Some(parent) = at.thread.filter(|_| removed) {
            for (_, tl) in self.timelines.iter_mut().filter(|(t, _)| at.holds(t)) {
                drawn |= tl.drop_reply(parent);
            }
        }
        drawn
    }

    /// The newest page of a catch-up arrived as the answer `generation`: `false` when it is not
    /// one. Panes where the user chose a message keep their view.
    pub(super) fn on_caught_up(&mut self, generation: Generation, page: &Page, model: &Model) -> bool {
        if self.timelines.caught_up(&page.target, generation).is_none() {
            return false;
        }
        self.pin(std::slice::from_ref(&page.target));
        let shown = page.messages.iter().map(|m| shown(model, &page.target, m)).collect();
        if let Some(tl) = self.timelines.caught_up(&page.target, generation) {
            tl.catch_up(page, shown);
        }
        true
    }

    /// Ask for `target`'s newest page again, as a catch-up.
    pub(super) fn ask_catch_up(&mut self, target: &Target) {
        let command = Command::Catchup { target: target.clone(), limit: CATCHUP_PAGE };
        let id = self.requests.ask(Some(target.workspace().clone()), command);
        self.timelines.entry(target).ask_catch_up(id);
    }

    /// Live updates of `ws` came back after they were lost: read its open conversations again
    /// (one newest page each, [`CATCHUP_MAX`] at most, the one in focus first), then its open
    /// threads ([`CATCHUP_THREADS_MAX`] at most; not one read only in part, whose newest replies
    /// are not read anyway), so what was missed meanwhile shows. One loading, or caught up
    /// already, is left as it is.
    pub fn catch_up(&mut self, ws: &WorkspaceId) {
        let active = self.active().and_then(|id| self.pane(id)).map(|p| p.target.clone());
        let mut targets: Vec<Target> = self
            .timelines
            .iter()
            .filter(|(t, tl)| {
                t.workspace() == ws
                    && !tl.items.is_empty()
                    && !tl.cached
                    && !tl.cut
                    && tl.pending.is_none()
                    && tl.catchup.is_none()
            })
            .map(|(t, _)| t.clone())
            .collect();
        let thread = |t: &Target| match t {
            Target::Thread { thread, .. } => Some(*thread),
            Target::Conversation { .. } => None,
        };
        targets.sort_by_key(|t| (Some(t) != active.as_ref(), t.conversation().clone(), thread(t)));
        let (threads, conversations): (Vec<Target>, Vec<Target>) =
            targets.into_iter().partition(|t| thread(t).is_some());
        let wanted = conversations.into_iter().take(CATCHUP_MAX).chain(threads.into_iter().take(CATCHUP_THREADS_MAX));
        for target in wanted.collect::<Vec<_>>() {
            self.ask_catch_up(&target);
        }
    }
}
