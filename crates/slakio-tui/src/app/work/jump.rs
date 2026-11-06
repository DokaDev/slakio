//! A Slack link opened inside slakio: the pane on its conversation is opened (or the one showing
//! it focused), and the message it points at selected and marked for a moment ([`FLASH`]). When
//! that message is not among what is loaded, the conversation is read around it instead (one
//! request the user waits for, [`Command::Around`]): what was loaded goes, and when the newer
//! messages were not read with it the pane says so after the last one; `G` there reads the
//! newest again. A reply's link opens its thread beside the conversation, the reply selected.

use super::Work;
use crate::app::pane::PAGE;
use slakio_core::backend::{Command, Generation};
use slakio_core::layout::PaneId;
use slakio_core::model::{Target, Ts};
use std::time::{Duration, Instant};

/// How long the message a link opened stays marked.
pub const FLASH: Duration = Duration::from_secs(2);

impl Work {
    /// Open `conversation` at the message at `at` (selected once loaded): the pane, and the
    /// request reading around it when it is not loaded (the one on its way already, when the
    /// same link was opened again before its answer came: it is not asked twice).
    pub fn jump(&mut self, conversation: Target, at: Ts) -> (Option<PaneId>, Option<Generation>) {
        let tl = self.timelines.entry(&conversation);
        let waits = self.panes.values().any(|p| p.target == conversation && p.jump == Some(at));
        if let Some(pending) = tl.pending.filter(|_| waits) {
            let pane = self.open(conversation);
            self.fill();
            return (pane, Some(pending));
        }
        let asked = (!tl.holds(at)).then(|| {
            let command = Command::Around { target: conversation.clone(), at, limit: PAGE };
            let id = self.requests.ask(Some(conversation.workspace().clone()), command);
            self.timelines.entry(&conversation).read_around(id);
            id
        });
        let pane = self.open(conversation);
        if let Some(p) = pane.and_then(|id| self.panes.get_mut(&id)) {
            p.jump = Some(at);
        }
        self.fill();
        (pane, asked)
    }

    /// Open the thread under the message at `parent` of `conversation` beside it, at its reply
    /// `reply`; the conversation goes to `parent`. The thread's pane, and the request reading
    /// around `parent` when it is not loaded.
    pub fn jump_reply(&mut self, conversation: Target, parent: Ts, reply: Ts) -> (Option<PaneId>, Option<Generation>) {
        let (owner, asked) = self.jump(conversation.clone(), parent);
        let Some(owner) = owner else { return (None, asked) };
        let thread = Target::Thread {
            workspace: conversation.workspace().clone(),
            conversation: conversation.conversation().clone(),
            thread: parent,
        };
        let pane = match self.showing(&thread) {
            Some(id) => {
                self.activate(id);
                Some(id)
            }
            None => {
                self.activate(owner);
                self.show_beside(thread)
            }
        };
        if let Some(p) = pane.and_then(|id| self.panes.get_mut(&id)) {
            p.jump = Some(reply);
        }
        self.fill();
        (pane, asked)
    }

    /// `G`: the active pane's newest message; in a conversation whose newest messages were not
    /// read (a link read it around a message), they are read again first.
    pub fn newest(&mut self) {
        let target = self.focused().filter(|p| !p.is_thread() && self.timeline(p).cut).map(|p| p.target.clone());
        let Some(target) = target else { return self.with_pane(|p, tl| p.select_newest(tl)) };
        *self.timelines.entry(&target) = super::Timeline::default();
        // Every pane on it follows the newest again.
        for p in self.panes.values_mut().filter(|p| p.target == target) {
            (p.selected, p.visual, p.jump, p.flash) = (None, None, None, None);
            (p.older, p.to_root) = (false, false);
            p.bottom.set(None);
            p.held.set(None);
            p.fresh.set(None);
            p.seen.borrow_mut().clear();
        }
        self.fill();
    }

    /// A page of `target` failed: a pane waiting for it to select a link's message waits no more.
    pub(super) fn forget_jumps(&mut self, target: &Target) {
        for p in self.panes.values_mut().filter(|p| p.target == *target) {
            p.jump = None;
        }
    }

    /// The marks just put on a link's message last [`FLASH`] from `now`.
    pub fn arm_flash(&mut self, now: Instant) {
        for p in self.panes.values_mut() {
            if let Some((ts, None)) = p.flash {
                p.flash = Some((ts, Some(now + FLASH)));
            }
        }
    }

    /// The marks whose moment passed at `now` go. `true` when one went.
    pub fn expire_flash(&mut self, now: Instant) -> bool {
        let mut gone = false;
        for p in self.panes.values_mut() {
            if p.flash.is_some_and(|(_, until)| until.is_some_and(|u| u <= now)) {
                (p.flash, gone) = (None, true);
            }
        }
        gone
    }

    /// A link's message was not there (deleted, or not in what was read) when its pane landed
    /// on the nearest one since the last call: said once.
    pub fn take_missed(&mut self) -> bool {
        std::mem::take(&mut self.missed)
    }

    /// When a mark goes next.
    pub(super) fn flash_deadline(&self) -> Option<Instant> {
        self.panes.values().filter_map(|p| p.flash.and_then(|(_, until)| until)).min()
    }
}
