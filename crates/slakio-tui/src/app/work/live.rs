//! Live updates in the panes: a message posted, changed or deleted goes into every timeline that
//! shows where it belongs — the conversation, the thread it is a reply of — and the count of
//! replies under a thread's own message follows. Panes name their selection and anchor by
//! timestamp, and one where the user chose a message keeps its view, so nothing moves under the
//! user; a pane following the newest message keeps following it. After live updates were lost a while, the open conversations are read again,
//! one newest page each ([`Work::catch_up`]).

use super::{Work, shown};
use crate::app::model::Model;
use slakio_core::backend::Command;
use slakio_core::model::{ConversationId, Message, Target, Ts, WorkspaceId};

/// The most conversations of a workspace read again after live updates were lost.
pub const CATCHUP_MAX: usize = 8;

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
        for pane in self.panes.values().filter(|p| into.contains(&p.target) && p.selected.is_some()) {
            if pane.bottom.get().is_none() {
                pane.bottom.set(self.timelines.get(&pane.target).and_then(|tl| tl.items.last()).map(|m| m.ts));
            }
        }
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

    /// The message at `ts` was deleted at `at` (a reply, when `at.thread` says so). `true` when a
    /// pane's messages changed.
    pub fn on_live_deleted(&mut self, at: Place<'_>, ts: Ts) -> bool {
        let mut drawn = false;
        for (_, tl) in self.timelines.iter_mut().filter(|(t, _)| at.holds(t)) {
            drawn |= tl.remove(ts);
            if let Some(parent) = at.thread {
                drawn |= tl.drop_reply(parent);
            }
        }
        drawn
    }

    /// Live updates of `ws` came back after they were lost: read its open conversations again
    /// (one newest page each, [`CATCHUP_MAX`] at most, the one in focus first), so what was
    /// missed meanwhile shows. One loading, or caught up already, is left as it is.
    pub fn catch_up(&mut self, ws: &WorkspaceId) {
        let active = self.active().and_then(|id| self.pane(id)).map(|p| p.target.clone());
        let mut targets: Vec<Target> = self
            .timelines
            .iter()
            .filter(|(t, tl)| {
                matches!(t, Target::Conversation { workspace, .. } if workspace == ws)
                    && !tl.items.is_empty()
                    && !tl.cached
                    && tl.pending.is_none()
                    && tl.catchup.is_none()
            })
            .map(|(t, _)| t.clone())
            .collect();
        targets.sort_by_key(|t| (Some(t) != active.as_ref(), t.conversation().clone()));
        for target in targets.into_iter().take(CATCHUP_MAX) {
            let command = Command::Catchup { target: target.clone(), limit: CATCHUP_PAGE };
            let id = self.requests.ask(Some(ws.clone()), command);
            self.timelines.entry(&target).catchup = Some(id);
        }
    }
}
