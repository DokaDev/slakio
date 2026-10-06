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
//! The panes are kept by id in a registry and placed by a layout tree
//! ([`slakio_core::layout`]): the conversation alone, or split with the thread panel beside it.
//! The conversation is the tree's first pane, the thread panel its second. The work area also
//! keeps which pane is active (the one the keyboard goes to in the work area); it never moves
//! the keyboard itself: what it opens or closes it hands back, and the app moves the focus.
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
use crate::screen::THREAD_SHARE;
use slakio_core::backend::{Command, Generation, Page};
use slakio_core::layout::{Dir, Node, PaneId};
use slakio_core::model::{Message, Target};
use slakio_core::sanitize::{Safe, sanitize_block, sanitize_line};
use std::collections::HashMap;

/// Targets the back history keeps.
const HISTORY: usize = 50;

#[derive(Clone, Debug, Default)]
pub struct Work {
    panes: HashMap<PaneId, Pane>,
    /// Where the panes go; `None`: no pane open.
    layout: Option<Node>,
    /// The pane the keyboard goes to in the work area.
    active: Option<PaneId>,
    /// The last pane id handed out.
    last: u64,
    back: Vec<Target>,
    forward: Vec<Target>,
    /// The messages of the targets the panes show.
    pub timelines: TimelineStore,
    /// What is being written to the targets the panes show.
    pub drafts: DraftStore,
    /// The app's requests to the backend (the boot request too).
    pub requests: Requests,
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
    /// The layout of the panes, when one is open.
    pub fn layout(&self) -> Option<&Node> {
        self.layout.as_ref()
    }

    /// The open panes, in reading order.
    pub fn ids(&self) -> Vec<PaneId> {
        self.layout.as_ref().map(Node::leaves).unwrap_or_default()
    }

    /// The conversation's pane (the layout's first).
    pub fn main_id(&self) -> Option<PaneId> {
        self.ids().first().copied()
    }

    /// The thread panel (the layout's second pane).
    pub fn thread_id(&self) -> Option<PaneId> {
        self.ids().get(1).copied()
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.get(&id)
    }

    pub fn main(&self) -> Option<&Pane> {
        self.main_id().and_then(|id| self.panes.get(&id))
    }

    /// The active pane: the one the keyboard goes to in the work area.
    pub fn active(&self) -> Option<PaneId> {
        self.active
    }

    /// The active pane.
    pub fn focused(&self) -> Option<&Pane> {
        self.active.and_then(|id| self.panes.get(&id))
    }

    pub fn focused_mut(&mut self) -> Option<&mut Pane> {
        self.active.and_then(|id| self.panes.get_mut(&id))
    }

    /// Make pane `id` the active one (the app does, as it moves the focus); Insert mode goes
    /// along.
    pub fn activate(&mut self, id: PaneId) {
        let insert = self.insert();
        self.active = Some(id);
        self.set_insert(insert);
    }

    /// The active pane is in Insert mode.
    pub fn insert(&self) -> bool {
        self.focused().is_some_and(|p| p.insert)
    }

    /// Insert mode on or off for the active pane; no other pane is in it.
    pub fn set_insert(&mut self, on: bool) {
        for p in self.panes.values_mut() {
            p.insert = false;
        }
        if let Some(p) = self.focused_mut() {
            p.insert = on;
        }
    }

    /// The messages pane `p` shows.
    pub fn timeline(&self, p: &Pane) -> &Timeline {
        self.timelines.get(&p.target).unwrap_or(&EMPTY)
    }

    /// What is being written in pane `p`.
    pub fn draft(&self, p: &Pane) -> &Composer {
        self.drafts.get(&p.target).unwrap_or(&NO_DRAFT)
    }

    /// The draft of the active pane, to write in.
    pub fn draft_mut(&mut self) -> Option<&mut Composer> {
        let target = self.focused()?.target.clone();
        Some(self.drafts.entry(&target))
    }

    /// The page requests queued since the last call.
    pub fn take_requests(&mut self) -> Vec<(Generation, Command)> {
        self.requests.take()
    }

    /// Put `pane` in the registry; its new id.
    fn add(&mut self, pane: Pane) -> PaneId {
        self.last += 1;
        let id = PaneId(self.last);
        self.panes.insert(id, pane);
        id
    }

    /// Take pane `id` out of the registry and the layout.
    fn remove(&mut self, id: PaneId) -> Option<Pane> {
        self.layout = self.layout.as_ref().and_then(|t| t.without(id));
        if self.active == Some(id) {
            self.active = None;
        }
        self.panes.remove(&id)
    }

    /// Ask for the pages the panes want; one request for a target however many panes show it.
    fn fill(&mut self) {
        for id in self.ids() {
            let Some(pane) = self.panes.get(&id) else { continue };
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
        let shown: Vec<Target> = self.panes.values().map(|p| p.target.clone()).collect();
        self.timelines.retain(|t| shown.contains(t));
        self.drafts.retain(|t, c| shown.contains(t) || !c.is_empty());
    }

    /// Open the conversation `target` (or find it open already); the pane to focus.
    pub fn open(&mut self, target: Target) -> Option<PaneId> {
        if let Some(current) = self.main().map(|p| p.target.clone()) {
            if current == target {
                return self.main_id();
            }
            self.back.push(current);
            if self.back.len() > HISTORY {
                self.back.remove(0);
            }
        }
        self.forward.clear();
        Some(self.show(target))
    }

    fn show(&mut self, target: Target) -> PaneId {
        self.panes.clear();
        self.layout = None;
        self.active = None;
        self.prune();
        let id = self.add(Pane::new(target));
        self.layout = Some(Node::Leaf(id));
        self.active = Some(id);
        self.fill();
        id
    }

    /// Back to the conversation before (`Ctrl+O`); its pane, `None` when there is none.
    pub fn back(&mut self) -> Option<PaneId> {
        let to = self.back.pop()?;
        if let Some(p) = self.main() {
            self.forward.push(p.target.clone());
        }
        Some(self.show(to))
    }

    /// Forward again (`Ctrl+I`); its pane, `None` when there is none.
    pub fn forward(&mut self) -> Option<PaneId> {
        let to = self.forward.pop()?;
        if let Some(p) = self.main() {
            self.back.push(p.target.clone());
        }
        Some(self.show(to))
    }

    /// The thread of the selected message of the conversation, in the thread panel (replacing
    /// what it showed, or found there already); the pane to focus.
    pub fn open_thread(&mut self) -> Option<PaneId> {
        if self.active != self.main_id() {
            return None;
        }
        let main = self.main()?;
        let tl = self.timeline(main);
        let m = main.selected_index(tl).and_then(|i| tl.items.get(i))?;
        let target = main.thread_target(m.ts);
        if let Some(id) = self.thread_id().filter(|id| self.panes.get(id).is_some_and(|t| t.target == target)) {
            return Some(id);
        }
        self.show_beside(target)
    }

    /// Show `target` in the thread panel beside the conversation (what it showed is closed);
    /// the pane to focus, which takes Insert mode along.
    pub fn show_beside(&mut self, target: Target) -> Option<PaneId> {
        let main = self.main_id()?;
        let insert = self.insert();
        self.set_insert(false);
        if let Some(old) = self.thread_id() {
            self.remove(old);
        }
        self.prune();
        let id = self.add(Pane { insert, ..Pane::new(target) });
        self.layout = Some(Node::split(Dir::Row, THREAD_SHARE, Node::Leaf(main), Node::Leaf(id)));
        self.active = Some(id);
        self.fill();
        Some(id)
    }

    /// Close the active pane (`Ctrl+W`): the thread panel (the conversation then selects the
    /// thread's message, and is the pane to focus), else the conversation, whose target is handed
    /// back.
    pub fn close(&mut self) -> (Option<Target>, Option<PaneId>) {
        self.set_insert(false);
        let Some(active) = self.active else { return (None, None) };
        if Some(active) == self.thread_id() {
            let closed = self.remove(active);
            let main = self.main_id();
            if let (Some(t), Some(main)) = (closed, main.and_then(|id| self.panes.get_mut(&id)))
                && let Target::Thread { thread, .. } = t.target
                && self.timelines.get(&main.target).is_some_and(|tl| tl.position(thread).is_some())
            {
                main.selected = Some(thread);
            }
            self.prune();
            return (None, main);
        }
        let closed = self.panes.get(&active).map(|p| p.target.clone());
        self.panes.clear();
        self.layout = None;
        self.active = None;
        self.prune();
        (closed, None)
    }

    /// Any draft holds text that was not sent, open in a pane or not.
    pub fn unsent(&self) -> bool {
        self.drafts.unsent()
    }

    /// The pane on the left (`-1`) or right (`1`) of the active one; `None` when there is none
    /// that way (the shell moves the focus out of the work area then).
    pub fn beside(&self, step: i8) -> Option<PaneId> {
        let ids = self.ids();
        let at = ids.iter().position(|id| Some(*id) == self.active)?;
        let to = at.checked_add_signed(isize::from(step))?;
        ids.get(to).copied()
    }

    /// A page of messages arrived. `true` when a timeline took it; an answer to an older
    /// request or for a target no pane shows is dropped. Every pane on the target keeps its
    /// selection on its messages (it names them by timestamp).
    pub fn on_page(&mut self, generation: Generation, page: &Page, model: &Model) -> bool {
        let Some(tl) = self.timelines.awaiting(&page.target, generation) else { return false };
        let shown = page.messages.iter().map(|m| shown(model, &page.target, m)).collect();
        tl.add_page(page, shown);
        for pane in self.panes.values_mut() {
            if pane.target == page.target {
                pane.arrived(tl);
            }
        }
        self.fill();
        true
    }

    /// Run `f` on the active pane and its timeline, then ask for the pages it now wants.
    pub fn with_pane(&mut self, f: impl FnOnce(&mut Pane, &Timeline)) {
        if let Some(p) = self.active.and_then(|id| self.panes.get_mut(&id)) {
            f(p, self.timelines.get(&p.target).unwrap_or(&EMPTY));
        }
        self.fill();
    }

    /// Move the active pane's selection by `by` messages.
    pub fn step(&mut self, by: isize) {
        self.with_pane(|p, tl| p.step(by, tl));
    }

    /// Move the selection of pane `id`, if any, by `by` messages (the mouse wheel).
    pub fn step_in(&mut self, id: Option<PaneId>, by: isize) {
        if let Some(p) = id.and_then(|id| self.panes.get_mut(&id)) {
            p.step(by, self.timelines.get(&p.target).unwrap_or(&EMPTY));
        }
        self.fill();
    }

    /// Run `f` on the active pane's draft, then ask for the pages the panes want.
    pub fn with_draft(&mut self, f: impl FnOnce(&mut Composer)) {
        if let Some(c) = self.draft_mut() {
            f(c);
        }
        self.fill();
    }

    /// Send what the active pane's composer holds. The demo only echoes it locally: it is
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

    /// Drop what no longer exists after a new boot answer. `true` when the panes were closed.
    pub fn clamp(&mut self, model: &Model) -> bool {
        let gone = self.main().is_some_and(|p| model.target(&p.target).is_none());
        if gone {
            self.panes.clear();
            self.layout = None;
            self.active = None;
            self.prune();
        }
        self.back.retain(|t| model.target(t).is_some());
        self.forward.retain(|t| model.target(t).is_some());
        // A draft of a conversation that is gone stays (quitting still asks about it): text the
        // user wrote is never dropped silently.
        gone
    }
}

#[cfg(test)]
mod tests;
