//! A pane: a view of one conversation or thread — the selection, a VISUAL range, Insert mode,
//! where the view is anchored and the rows drawn last. The messages are not the pane's: they are the
//! target's [`Timeline`], shared by every pane that shows it, and what is being written is the
//! target's draft ([`super::drafts`]). A pane asks for older messages as its selection nears
//! the top; drawing lays out only the rows on screen.
//!
//! The selection, the VISUAL range and the anchor name messages by their timestamp, so they stay
//! on their messages whatever arrives before or after them.
//!
//! The view is anchored at the bottom: `bottom` is the message on the last row, or the newest
//! while nothing is selected (so new messages stay in view). Drawing moves `bottom` to keep the
//! selection on screen; it is a `Cell` because only drawing knows the rows' heights. At the top
//! of a history longer than the screen (`gg`), the oldest message is on the first row and newer
//! ones below `bottom` fill what is left; a history that fits whole sits at the bottom.

use super::timelines::Timeline;
use slakio_core::model::{Message, Target, ThreadSummary, Ts, UserId};
use slakio_core::sanitize::Safe;
use std::cell::{Cell, RefCell};

/// Messages asked for at a time.
pub const PAGE: u32 = 200;
/// Older messages are asked for when the selection comes this close to the oldest loaded one.
pub const PREFETCH: usize = 30;

/// A reaction as drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownReaction {
    pub name: Safe,
    pub count: u32,
    pub mine: bool,
}

/// A message as drawn: every remote text already sanitised.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub ts: Ts,
    pub user: UserId,
    pub author: Safe,
    /// Written by the user.
    pub own: bool,
    pub text: Safe,
    pub thread: Option<ThreadSummary>,
    pub reactions: Vec<ShownReaction>,
    pub edited: bool,
}

impl Shown {
    /// `m` as drawn, by `author` (the sender's name, sanitised), `own` when the user wrote it.
    pub fn new(m: &Message, author: Safe, own: bool) -> Self {
        Self {
            ts: m.ts,
            user: m.user.clone(),
            author,
            own,
            text: m.text.block(),
            thread: m.thread,
            reactions: m
                .reactions
                .iter()
                .map(|r| ShownReaction { name: r.name.line(), count: r.count, mine: r.mine })
                .collect(),
            edited: m.edited,
        }
    }
}

/// A row of a pane as last drawn, for the mouse: the message on it, and whether it is the
/// message's "N replies" link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub y: u16,
    pub message: usize,
    pub link: bool,
}

#[derive(Clone, Debug)]
pub struct Pane {
    pub target: Target,
    /// The selected message, by its timestamp; `None` until the user moves.
    pub selected: Option<Ts>,
    /// The other end of the VISUAL range, while selecting one.
    pub visual: Option<Ts>,
    /// The message on the last row (`None`: the newest). Kept by drawing.
    pub bottom: Cell<Option<Ts>>,
    /// `gg` was pressed before the oldest message was loaded: keep loading, then select it.
    pub to_oldest: bool,
    /// Its composer is being written in (Insert mode); only the pane with the keyboard is.
    pub insert: bool,
    /// The rows drawn last, by drawing (only it knows the rows' heights).
    pub hits: RefCell<Vec<Hit>>,
}

impl Pane {
    pub fn new(target: Target) -> Self {
        Self {
            target,
            selected: None,
            visual: None,
            bottom: Cell::new(None),
            to_oldest: false,
            insert: false,
            hits: RefCell::new(Vec::new()),
        }
    }

    /// The thread under `ts` in this pane's conversation.
    pub fn thread_target(&self, ts: Ts) -> Target {
        Target::Thread {
            workspace: self.target.workspace().clone(),
            conversation: self.target.conversation().clone(),
            thread: ts,
        }
    }

    pub fn is_thread(&self) -> bool {
        matches!(self.target, Target::Thread { .. })
    }

    /// The page of `tl` (this pane's timeline) to ask for next, if one is wanted: the newest
    /// first, then older ones when the selection nears the top. `gg` waiting for the oldest
    /// needs no case of its own: it keeps the oldest loaded message selected, which is near the
    /// top.
    pub fn wants(&self, tl: &Timeline) -> Option<Option<Ts>> {
        if tl.pending.is_some() || tl.complete {
            return None;
        }
        let Some(first) = tl.items.first() else { return Some(None) };
        let near_top = self.selected_index(tl).is_some_and(|s| s < PREFETCH);
        near_top.then_some(Some(first.ts))
    }

    /// A page arrived in `tl` (this pane's timeline): `gg` keeps the oldest selected until it
    /// is loaded. (The selection names its message, so it stays on it.)
    pub fn arrived(&mut self, tl: &Timeline) {
        if self.to_oldest {
            self.selected = tl.items.first().map(|m| m.ts);
            self.to_oldest = !tl.complete;
        }
    }

    /// The selected message's index in `tl` (this pane's timeline).
    pub fn selected_index(&self, tl: &Timeline) -> Option<usize> {
        self.selected.and_then(|ts| tl.index_near(ts))
    }

    /// Select message `i` of `tl` (this pane's timeline).
    pub fn select_index(&mut self, i: usize, tl: &Timeline) {
        self.selected = tl.items.get(i).map(|m| m.ts);
    }

    /// Select the next (`1`) or previous (`-1`) message of `tl` (this pane's timeline); the
    /// first move selects the newest.
    pub fn step(&mut self, by: isize, tl: &Timeline) {
        self.to_oldest = false;
        let Some(last) = tl.items.len().checked_sub(1) else { return };
        let i = match self.selected_index(tl) {
            None => last,
            Some(s) => s.saturating_add_signed(by).min(last),
        };
        self.select_index(i, tl);
    }

    /// Select the newest message of `tl` (this pane's timeline).
    pub fn select_newest(&mut self, tl: &Timeline) {
        self.to_oldest = false;
        self.selected = tl.items.last().map(|m| m.ts);
    }

    /// The message this thread pane is the thread of, when loaded in `tl` (its timeline).
    pub fn root(&self, tl: &Timeline) -> Option<usize> {
        let Target::Thread { thread, .. } = &self.target else { return None };
        tl.position(*thread)
    }

    /// Select the message on screen row `y` as last drawn; `Some(link)` when there is one.
    pub fn hit(&self, y: u16) -> Option<Hit> {
        self.hits.borrow().iter().find(|h| h.y == y).copied()
    }

    /// Select the oldest message of `tl` (its timeline), loading the rest of the history first.
    pub fn select_oldest(&mut self, tl: &Timeline) {
        if tl.items.is_empty() {
            return;
        }
        self.select_index(0, tl);
        self.to_oldest = !tl.complete;
    }

    /// The selected messages of `tl` (this pane's timeline): the VISUAL range, or the selected
    /// one, as indices.
    pub fn range(&self, tl: &Timeline) -> Option<(usize, usize)> {
        let s = self.selected_index(tl)?;
        let v = self.visual.and_then(|ts| tl.index_near(ts)).unwrap_or(s);
        Some((s.min(v), s.max(v)))
    }

    /// The user's own message was added after the newest: back to the newest, so it is in
    /// view.
    pub fn echoed(&mut self) {
        self.selected = None;
        self.visual = None;
        self.bottom.set(None);
    }
}

/// The user's own message (the demo's local echo), after the newest of `items`.
pub fn echo(items: &mut Vec<Shown>, user: UserId, author: Safe, text: Safe) {
    let ts = items.last().map_or(Ts(0), |m| Ts(m.ts.0 + 1_000_000));
    items.push(Shown { ts, user, author, own: true, text, thread: None, reactions: Vec::new(), edited: false });
}

#[cfg(test)]
pub(crate) mod tests;
