//! The work area: tabs ([`slakio_core::layout::tabs`]), each a layout of panes; the one shown
//! is the default GUI Slack mode while it is the only one — the conversation open in the main
//! pane, the auto thread panel beside it, which of the two has the keyboard and whether its
//! composer is being written in. Each pane keeps its own back/forward history. Closed tabs are
//! kept (the last [`tabs::REOPEN`]) to be opened again where they were ([`tabs`]), so `Ctrl+O` in an
//! empty work area opens the tab closed last.
//!
//! ```text
//! ╭ #backend ───────────────╮╭ ⤷ Thread ─────────╮
//! │ Kim  Starting deploy    ││ Kim  Starting …   │
//! │      ⤷ 4 replies        ││ Park Confirmed    │
//! │╭ Message #backend ─────╮││╭ Reply ─────────╮ │
//! ```
//!
//! `Enter` on a message opens its thread in the panel, replacing what it showed. Opening what is
//! open already, in any tab, focuses it instead of opening it twice.
//!
//! The panes are kept by id in a registry and placed by a layout tree
//! ([`slakio_core::layout`]): the conversation alone, or split with the thread panel beside it.
//! What a pane is does not come from its place in the tree: a pane that opened the auto thread
//! panel names it ([`Pane::thread`]); the panel is the pane some other pane names, and closing
//! that relation (closing either pane) is all it takes to make it an ordinary pane or none. The
//! work area also keeps which pane is active (the one the keyboard goes to in the work area); it
//! never moves the keyboard itself: what it opens or closes it hands back, and the app moves the
//! focus.
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
use super::pane::{History, PAGE, Pane, Shown, echo};
use super::requests::Requests;
use super::timelines::{Timeline, TimelineStore};
use crate::screen::THREAD_SHARE;
use slakio_core::backend::{Command, Generation, Page};
use slakio_core::layout::tabs::{Tab, Tabs};
use slakio_core::layout::{Dir, Node, PaneId};
use slakio_core::model::{Message, Target};
use slakio_core::sanitize::{Safe, sanitize_block, sanitize_line};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct Work {
    panes: HashMap<PaneId, Pane>,
    /// Where the panes go, by tab; each tab's active pane is the one the keyboard goes to there.
    tabs: Tabs,
    /// The last pane id handed out.
    last: u64,
    /// The tabs closed, to open again; the newest last.
    closed: Vec<ClosedTab>,
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
    /// The layout of the tab shown, when there is one.
    pub fn layout(&self) -> Option<&Node> {
        self.tabs.current_tab().map(|t| &t.root)
    }

    /// The tabs.
    pub fn tabs(&self) -> &Tabs {
        &self.tabs
    }

    /// The panes of the tab shown, in reading order.
    pub fn ids(&self) -> Vec<PaneId> {
        self.layout().map(Node::leaves).unwrap_or_default()
    }

    /// The panes of the tab shown that some pane names as its thread panel.
    fn panel_shown(&self) -> bool {
        self.ids().into_iter().any(|id| self.owner(id).is_some())
    }

    /// The pane that opened the thread panel `id`; `None` when `id` is no pane's thread panel.
    pub fn owner(&self, id: PaneId) -> Option<PaneId> {
        self.panes.iter().find(|(_, p)| p.thread == Some(id)).map(|(owner, _)| *owner)
    }

    /// A pane of the tab shown is some pane's auto thread panel.
    pub fn has_panel(&self) -> bool {
        self.panel_shown()
    }

    /// The pane what the list opens goes to: the active pane, or the pane that opened it when it
    /// is a thread panel.
    pub fn home_id(&self) -> Option<PaneId> {
        self.active().map(|a| self.owner(a).unwrap_or(a))
    }

    /// The open pane that shows `target`, in any tab (the tab shown first).
    pub fn showing(&self, target: &Target) -> Option<PaneId> {
        let shown = self.ids();
        let all = shown.iter().copied().chain(self.tabs.panes().into_iter().filter(|id| !shown.contains(id)));
        all.into_iter().find(|id| self.panes.get(id).is_some_and(|p| &p.target == target))
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.get(&id)
    }

    /// The pane what the list opens goes to ([`Self::home_id`]).
    pub fn home(&self) -> Option<&Pane> {
        self.home_id().and_then(|id| self.panes.get(&id))
    }

    /// The active pane of the tab shown: the one the keyboard goes to in the work area.
    pub fn active(&self) -> Option<PaneId> {
        self.tabs.current_tab().map(|t| t.active)
    }

    /// The active pane.
    pub fn focused(&self) -> Option<&Pane> {
        self.active().and_then(|id| self.panes.get(&id))
    }

    pub fn focused_mut(&mut self) -> Option<&mut Pane> {
        self.active().and_then(|id| self.panes.get_mut(&id))
    }

    /// Make pane `id` the active one, showing its tab (the app does, as it moves the focus);
    /// Insert mode goes along. A pane that is not open is not made active.
    pub fn activate(&mut self, id: PaneId) {
        let insert = self.insert();
        self.point(id);
        self.set_insert(insert);
        self.check();
    }

    /// Show the tab of pane `id` with `id` its active pane.
    fn point(&mut self, id: PaneId) {
        let Some(i) = self.tabs.find(id) else { return };
        self.show_tab(i);
        if let Some(t) = self.tabs.current_tab_mut() {
            t.active = id;
        }
    }

    /// Show tab `i` (its panes ask for the pages they want).
    fn show_tab(&mut self, i: usize) {
        if self.tabs.current() != Some(i) && self.tabs.select(i) {
            self.set_insert(false);
            self.fill();
        }
    }

    /// What always holds, checked after every change in debug builds: the tabs hold
    /// ([`Tabs::holds`]); the panes open are those the tabs place, each once; a thread panel a
    /// pane names is open, in the same tab.
    fn check(&self) {
        let placed = self.tabs.panes();
        debug_assert!(self.tabs.holds(), "the tabs hold: {:?}", self.tabs);
        debug_assert_eq!(placed.len(), self.panes.len(), "every pane open is placed, once");
        debug_assert!(placed.iter().all(|id| self.panes.contains_key(id)), "every pane placed is open");
        debug_assert!(
            self.panes
                .iter()
                .filter_map(|(id, p)| p.thread.map(|t| (*id, t)))
                .all(|(id, t)| self.panes.contains_key(&t) && self.tabs.find(t) == self.tabs.find(id)),
            "a thread panel named is open, beside its pane"
        );
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

    /// Take pane `id` out of the registry and its tab (a tab left empty closes, unrecorded);
    /// where it was its tab's active pane, `next` is (or the tab's first pane).
    fn remove(&mut self, id: PaneId, next: Option<PaneId>) -> Option<Pane> {
        self.tabs.close_pane(id, next);
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

    /// Open the conversation `target` in the pane the list opens into, which remembers what it
    /// showed (with no tab open, in a new tab); what is open already, in any tab, is focused
    /// instead. The pane to focus.
    pub fn open(&mut self, target: Target) -> Option<PaneId> {
        if let Some(id) = self.showing(&target) {
            self.activate(id);
            return Some(id);
        }
        let Some(id) = self.home_id() else { return Some(self.new_tab(target, History::default())) };
        let pane = self.panes.get_mut(&id)?;
        pane.history.left(pane.target.clone());
        self.retarget(id, target);
        Some(id)
    }

    /// A new tab, right after the one shown, of one pane showing `target` with `history`; the
    /// pane.
    fn new_tab(&mut self, target: Target, history: History) -> PaneId {
        self.set_insert(false);
        let id = self.add(Pane { history, ..Pane::new(target) });
        self.tabs.open(Tab::new(id));
        self.prune();
        self.fill();
        self.check();
        id
    }

    /// Pane `id` shows `target` instead of what it showed, from scratch; its thread panel closes.
    fn retarget(&mut self, id: PaneId, target: Target) {
        if let Some(panel) = self.panes.get_mut(&id).and_then(|p| p.thread.take()) {
            self.remove(panel, Some(id));
        }
        if let Some(p) = self.panes.get_mut(&id) {
            p.show(target);
        }
        self.point(id);
        self.prune();
        self.fill();
        self.check();
    }

    /// Back to what the pane the list opens into showed before (`Ctrl+O`); in an empty work area,
    /// the tab closed last again. The pane, `None` when there is nothing back.
    pub fn back(&mut self) -> Option<PaneId> {
        let Some(id) = self.home_id() else { return self.reopen() };
        let pane = self.panes.get_mut(&id)?;
        let to = pane.history.back(&pane.target)?;
        self.retarget(id, to);
        Some(id)
    }

    /// Forward again (`Ctrl+I`); the pane, `None` when there is nothing forward.
    pub fn forward(&mut self) -> Option<PaneId> {
        let id = self.home_id()?;
        let pane = self.panes.get_mut(&id)?;
        let to = pane.history.forward(&pane.target)?;
        self.retarget(id, to);
        Some(id)
    }

    /// The thread of the selected message of the active pane's conversation, in the pane's
    /// thread panel (replacing what it showed, or found there already); the pane to focus. A
    /// thread has no threads of its own.
    pub fn open_thread(&mut self) -> Option<PaneId> {
        let target = self.selected_thread()?;
        if let Some(id) = self.showing(&target) {
            self.activate(id);
            return Some(id);
        }
        self.show_beside(target)
    }

    /// The thread of the active pane's selected message (a conversation's; a thread has no
    /// threads of its own).
    pub fn selected_thread(&self) -> Option<Target> {
        let pane = self.focused().filter(|p| !p.is_thread())?;
        let tl = self.timeline(pane);
        let m = pane.selected_index(tl).and_then(|i| tl.items.get(i))?;
        Some(pane.thread_target(m.ts))
    }

    /// Show `target` in the thread panel of the pane what the list opens goes to (what the panel
    /// showed is closed); the pane to focus, which takes Insert mode along.
    pub fn show_beside(&mut self, target: Target) -> Option<PaneId> {
        let owner = self.home_id()?;
        let insert = self.insert();
        self.set_insert(false);
        if let Some(old) = self.panes.get_mut(&owner).and_then(|p| p.thread.take()) {
            self.remove(old, Some(owner));
        }
        self.prune();
        let id = self.add(Pane { insert, ..Pane::new(target) });
        let beside = Node::split(Dir::Row, THREAD_SHARE, Node::Leaf(owner), Node::Leaf(id));
        let tab = self.tabs.current_tab_mut()?;
        tab.root = tab.root.replace(owner, &beside);
        tab.active = id;
        if let Some(p) = self.panes.get_mut(&owner) {
            p.thread = Some(id);
        }
        self.fill();
        self.check();
        Some(id)
    }

    /// Close the active pane (`Ctrl+W`), and the thread panel it opened; closing the last pane
    /// of a tab closes the tab ([`Self::close_tab`]). A thread panel hands the keyboard back to
    /// the pane that opened it, which selects the thread's message. The pane to focus next, else
    /// (no tab left) the target of the pane closed.
    pub fn close(&mut self) -> (Option<Target>, Option<PaneId>) {
        self.set_insert(false);
        let Some(active) = self.active() else { return (None, None) };
        if let Some(owner) = self.owner(active) {
            let closed = self.remove(active, Some(owner));
            if let (Some(t), Some(pane)) = (closed, self.panes.get_mut(&owner)) {
                pane.thread = None;
                if let Target::Thread { thread, .. } = t.target
                    && self.timelines.get(&pane.target).is_some_and(|tl| tl.position(thread).is_some())
                {
                    pane.selected = Some(thread);
                }
            }
            self.prune();
            self.check();
            return (None, Some(owner));
        }
        let panel = self.panes.get(&active).and_then(|p| p.thread);
        let alone = self.ids().iter().all(|id| *id == active || Some(*id) == panel);
        if alone && let Some(i) = self.tabs.current() {
            let closed = self.close_tab(i);
            return match self.active() {
                Some(next) => (None, Some(next)),
                None => (closed, None),
            };
        }
        self.close_pane(active);
        self.prune();
        self.check();
        (None, self.active())
    }

    /// Take pane `id` and the thread panel it opened out (a tab left empty closes, unrecorded).
    fn close_pane(&mut self, id: PaneId) -> Option<Pane> {
        let pane = self.remove(id, None)?;
        if let Some(panel) = pane.thread {
            self.remove(panel, None);
        }
        if let Some(owner) = self.owner(id).and_then(|o| self.panes.get_mut(&o)) {
            owner.thread = None;
        }
        Some(pane)
    }

    /// Any draft holds text that was not sent, open in a pane or not.
    pub fn unsent(&self) -> bool {
        self.drafts.unsent()
    }

    /// The pane on the left (`-1`) or right (`1`) of the active one; `None` when there is none
    /// that way (the shell moves the focus out of the work area then).
    pub fn beside(&self, step: i8) -> Option<PaneId> {
        let ids = self.ids();
        let at = ids.iter().position(|id| Some(*id) == self.active())?;
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
        if let Some(p) = self.active().and_then(|id| self.panes.get_mut(&id)) {
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

    /// Drop what no longer exists after a new boot answer: the panes of a conversation that is
    /// gone close. `true` when a pane was closed.
    pub fn clamp(&mut self, model: &Model) -> bool {
        let gone: Vec<PaneId> = self
            .tabs
            .panes()
            .into_iter()
            .filter(|id| self.panes.get(id).is_some_and(|p| model.target(&p.target).is_none()))
            .collect();
        for id in &gone {
            self.close_pane(*id);
        }
        self.prune();
        self.check();
        let gone = !gone.is_empty();
        let exists = |t: &Target| model.target(t).is_some();
        for p in self.panes.values_mut() {
            p.history.retain(exists);
        }
        self.closed.retain_mut(|c| c.retain(exists));
        // A draft of a conversation that is gone stays (quitting still asks about it): text the
        // user wrote is never dropped silently.
        gone
    }
}

mod tabs;
use tabs::ClosedTab;

#[cfg(test)]
mod tests;
