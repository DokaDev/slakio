//! The work area in the default (GUI Slack) mode: the conversation open in the main pane, the
//! auto thread panel beside it, which of the two has the keyboard, whether its composer is
//! being written in, and the back/forward history of the main pane.
//!
//! ```text
//! ╭ #backend ───────────────╮╭ ⤷ Thread ─────────╮
//! │ Kim  Starting deploy    ││ Kim  Starting …   │
//! │      ⤷ 4 replies        ││ Park Confirmed    │
//! │╭ Message #backend ─────╮││╭ Reply ─────────╮ │
//! ```
//!
//! `Enter` on a message opens its thread in the panel, replacing what it showed. Opening a
//! conversation that is already open focuses it instead of loading it again.
//!
//! The panes are views. What they show is held once per target, outside them: the messages in
//! the [`TimelineStore`], what is being written in the [`DraftStore`]. The messages of a target no
//! pane shows any more are dropped, so opening it again loads it afresh; its draft stays, and is
//! there again when it is opened. Pages are asked for through
//! the app's one [`Requests`] allocator; an answer for an older request or another target is
//! dropped.

use super::composer::Composer;
use super::drafts::DraftStore;
use super::model::Model;
use super::pane::{PAGE, Pane, Shown, echo};
use super::requests::Requests;
use super::timelines::{Timeline, TimelineStore};
use crate::screen::Slot;
use slakio_core::backend::{Command, Generation, Page};
use slakio_core::model::{Message, Target};
use slakio_core::sanitize::{Safe, sanitize_block, sanitize_line};

/// Targets the back history keeps.
const HISTORY: usize = 50;

/// One of the two panes of the work area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Main,
    Thread,
}

impl Side {
    /// Where the pane on this side is laid out.
    pub fn slot(self) -> Slot {
        match self {
            Side::Main => Slot::Main,
            Side::Thread => Slot::Thread,
        }
    }

    /// The side of the pane laid out in `slot`.
    pub fn of(slot: Slot) -> Self {
        match slot {
            Slot::Main => Side::Main,
            Slot::Thread => Side::Thread,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Work {
    pub main: Option<Pane>,
    /// The auto thread panel.
    pub thread: Option<Pane>,
    /// Which pane has the keyboard when the work area has the focus.
    pub side: Side,
    back: Vec<Target>,
    forward: Vec<Target>,
    /// The messages of the targets the panes show.
    pub timelines: TimelineStore,
    /// What is being written to the targets the panes show.
    pub drafts: DraftStore,
    /// The app's requests to the backend (the boot request too).
    pub requests: Requests,
}

impl Default for Work {
    fn default() -> Self {
        Self {
            main: None,
            thread: None,
            side: Side::Main,
            back: Vec::new(),
            forward: Vec::new(),
            timelines: TimelineStore::default(),
            drafts: DraftStore::default(),
            requests: Requests::default(),
        }
    }
}

/// The draft of a target that has none yet.
static NO_DRAFT: Composer = Composer::EMPTY;
/// The timeline of a target that has none yet.
static EMPTY: Timeline = Timeline { items: Vec::new(), complete: false, pending: None };

/// `m` as drawn in a pane of `model`.
fn shown(model: &Model, target: &Target, m: &Message) -> Shown {
    let author = model.user(&m.user).map_or_else(|| sanitize_line(m.user.as_str()), |u| u.display_name.line());
    let own = model.me(target.workspace()) == Some(&m.user);
    Shown::new(m, author, own)
}

impl Work {
    /// The pane with the keyboard.
    pub fn focused(&self) -> Option<&Pane> {
        match self.side {
            Side::Thread => self.thread.as_ref(),
            Side::Main => self.main.as_ref(),
        }
    }

    /// The pane laid out in `slot`, when open.
    pub fn pane(&self, slot: Slot) -> Option<&Pane> {
        match slot {
            Slot::Main => self.main.as_ref(),
            Slot::Thread => self.thread.as_ref(),
        }
    }

    pub fn focused_mut(&mut self) -> Option<&mut Pane> {
        match self.side {
            Side::Thread => self.thread.as_mut(),
            Side::Main => self.main.as_mut(),
        }
    }

    /// The focused pane is in Insert mode.
    pub fn insert(&self) -> bool {
        self.focused().is_some_and(|p| p.insert)
    }

    /// Insert mode on or off for the focused pane; no other pane is in it.
    pub fn set_insert(&mut self, on: bool) {
        for p in [self.main.as_mut(), self.thread.as_mut()].into_iter().flatten() {
            p.insert = false;
        }
        if let Some(p) = self.focused_mut() {
            p.insert = on;
        }
    }

    /// Give the keyboard to the pane on `side`; Insert mode goes along.
    pub fn turn(&mut self, side: Side) {
        let insert = self.insert();
        self.side = side;
        self.set_insert(insert);
    }

    /// The messages pane `p` shows.
    pub fn timeline(&self, p: &Pane) -> &Timeline {
        self.timelines.get(&p.target).unwrap_or(&EMPTY)
    }

    /// What is being written in pane `p`.
    pub fn draft(&self, p: &Pane) -> &Composer {
        self.drafts.get(&p.target).unwrap_or(&NO_DRAFT)
    }

    /// The draft of the focused pane, to write in.
    pub fn draft_mut(&mut self) -> Option<&mut Composer> {
        let target = self.focused()?.target.clone();
        Some(self.drafts.entry(&target))
    }

    /// The page requests queued since the last call.
    pub fn take_requests(&mut self) -> Vec<(Generation, Command)> {
        self.requests.take()
    }

    /// Ask for the pages the panes want; one request for a target however many panes show it.
    fn fill(&mut self) {
        for pane in [self.main.as_ref(), self.thread.as_ref()].into_iter().flatten() {
            let tl = self.timelines.entry(&pane.target);
            if let Some(before) = pane.wants(tl) {
                let command = Command::History { target: pane.target.clone(), before, limit: PAGE };
                tl.pending = Some(self.requests.ask(command));
            }
        }
    }

    /// Drop the messages of the targets no pane shows, and the empty drafts. A draft with text
    /// stays for when its target is opened again.
    fn prune(&mut self) {
        let shown: Vec<Target> = [&self.main, &self.thread].into_iter().flatten().map(|p| p.target.clone()).collect();
        self.timelines.retain(|t| shown.contains(t));
        self.drafts.retain(|t, c| shown.contains(t) || !c.is_empty());
    }

    /// Open the conversation `target` in the main pane (focusing it if it is open already).
    pub fn open(&mut self, target: Target) {
        if let Some(current) = self.main.as_ref().map(|p| p.target.clone()) {
            if current == target {
                self.turn(Side::Main);
                return;
            }
            self.back.push(current);
            if self.back.len() > HISTORY {
                self.back.remove(0);
            }
        }
        self.forward.clear();
        self.show(target);
    }

    fn show(&mut self, target: Target) {
        self.main = None;
        self.thread = None;
        self.prune();
        self.main = Some(Pane::new(target));
        self.side = Side::Main;
        self.fill();
    }

    /// Back to the conversation before (`Ctrl+O`). `false` when there is none.
    pub fn back(&mut self) -> bool {
        let Some(to) = self.back.pop() else { return false };
        if let Some(p) = self.main.as_ref() {
            self.forward.push(p.target.clone());
        }
        self.show(to);
        true
    }

    /// Forward again (`Ctrl+I`). `false` when there is none.
    pub fn forward(&mut self) -> bool {
        let Some(to) = self.forward.pop() else { return false };
        if let Some(p) = self.main.as_ref() {
            self.back.push(p.target.clone());
        }
        self.show(to);
        true
    }

    /// Open the thread of the selected message of the main pane in the thread panel (replacing
    /// what it showed; focusing it when it shows that thread already).
    pub fn open_thread(&mut self) {
        if self.side != Side::Main {
            return;
        }
        let Some(main) = self.main.as_ref() else { return };
        let tl = self.timeline(main);
        let Some(m) = main.selected_index(tl).and_then(|i| tl.items.get(i)) else { return };
        let target = main.thread_target(m.ts);
        if self.thread.as_ref().is_some_and(|t| t.target == target) {
            self.turn(Side::Thread);
            return;
        }
        self.show_beside(target);
    }

    /// Show `target` in the thread panel, with the keyboard (what it showed is closed).
    pub fn show_beside(&mut self, target: Target) {
        let insert = self.insert();
        self.set_insert(false);
        self.thread = None;
        self.prune();
        self.thread = Some(Pane { insert, ..Pane::new(target) });
        self.side = Side::Thread;
        self.fill();
    }

    /// Close the focused pane (`Ctrl+W`): the thread panel (the main pane then selects the
    /// thread's message), else the conversation, which is handed back.
    pub fn close(&mut self) -> Option<Target> {
        self.set_insert(false);
        let closed = match self.side {
            Side::Thread => {
                let closed = self.thread.take();
                self.side = Side::Main;
                if let (Some(t), Some(main)) = (closed, self.main.as_mut())
                    && let Target::Thread { thread, .. } = t.target
                    && self.timelines.get(&main.target).is_some_and(|tl| tl.position(thread).is_some())
                {
                    main.selected = Some(thread);
                }
                None
            }
            Side::Main => {
                self.thread = None;
                self.main.take().map(|p| p.target)
            }
        };
        self.prune();
        closed
    }

    /// Any draft holds text that was not sent, open in a pane or not.
    pub fn unsent(&self) -> bool {
        self.drafts.unsent()
    }

    /// Move the keyboard to the pane on the left (`-1`) or right (`1`). `false` when there is
    /// none that way (the shell moves the focus out of the work area then).
    pub fn focus_side(&mut self, step: i8) -> bool {
        match (self.side, step) {
            (Side::Main, 1) if self.thread.is_some() => self.side = Side::Thread,
            (Side::Thread, -1) => self.side = Side::Main,
            _ => return false,
        }
        self.set_insert(false);
        true
    }

    /// A page of messages arrived. `true` when a timeline took it; an answer to an older
    /// request or for a target no pane shows is dropped. Every pane on the target keeps its
    /// selection on its messages (it names them by timestamp).
    pub fn on_page(&mut self, generation: Generation, page: &Page, model: &Model) -> bool {
        let Some(tl) = self.timelines.awaiting(&page.target, generation) else { return false };
        let shown = page.messages.iter().map(|m| shown(model, &page.target, m)).collect();
        tl.add_page(page, shown);
        for pane in [self.main.as_mut(), self.thread.as_mut()].into_iter().flatten() {
            if pane.target == page.target {
                pane.arrived(tl);
            }
        }
        self.fill();
        true
    }

    /// Run `f` on the focused pane and its timeline, then ask for the pages it now wants.
    pub fn with_pane(&mut self, f: impl FnOnce(&mut Pane, &Timeline)) {
        let pane = match self.side {
            Side::Thread => self.thread.as_mut(),
            Side::Main => self.main.as_mut(),
        };
        if let Some(p) = pane {
            f(p, self.timelines.get(&p.target).unwrap_or(&EMPTY));
        }
        self.fill();
    }

    /// Move the focused pane's selection by `by` messages.
    pub fn step(&mut self, by: isize) {
        self.with_pane(|p, tl| p.step(by, tl));
    }

    /// Move the selection of the pane in `slot`, if any, by `by` messages (the mouse wheel).
    pub fn step_in(&mut self, slot: Option<Slot>, by: isize) {
        let pane = match slot {
            Some(Slot::Thread) => self.thread.as_mut(),
            Some(Slot::Main) => self.main.as_mut(),
            None => None,
        };
        if let Some(p) = pane {
            p.step(by, self.timelines.get(&p.target).unwrap_or(&EMPTY));
        }
        self.fill();
    }

    /// Run `f` on the focused pane's draft, then ask for the pages the panes want.
    pub fn with_draft(&mut self, f: impl FnOnce(&mut Composer)) {
        if let Some(c) = self.draft_mut() {
            f(c);
        }
        self.fill();
    }

    /// Send what the focused pane's composer holds. The demo only echoes it locally: it is
    /// shown as the user's message and goes nowhere. `false` when there was nothing to send, or
    /// it could not be sent (the composer keeps it then).
    pub fn send(&mut self, model: &Model) -> bool {
        let Some(target) = self.focused().map(|p| p.target.clone()) else { return false };
        // Everything that can stop the send is checked before the text leaves the composer.
        let Some(me) = model.me(target.workspace()).cloned() else { return false };
        let draft = self.drafts.entry(&target);
        if draft.text().trim().is_empty() {
            return false;
        }
        let text = draft.take();
        let author = model.user(&me).map_or_else(Safe::default, |u| u.display_name.line());
        echo(&mut self.timelines.entry(&target).items, me, author, sanitize_block(&text));
        if let Some(p) = self.focused_mut() {
            p.echoed();
        }
        true
    }

    /// Drop what no longer exists after a new boot answer.
    pub fn clamp(&mut self, model: &Model) {
        if self.main.as_ref().is_some_and(|p| model.target(&p.target).is_none()) {
            self.main = None;
            self.thread = None;
            self.side = Side::Main;
            self.prune();
        }
        self.back.retain(|t| model.target(t).is_some());
        self.forward.retain(|t| model.target(t).is_some());
        // A draft of a conversation that is gone stays (quitting still asks about it): text the
        // user wrote is never dropped silently.
    }
}

#[cfg(test)]
mod tests;
