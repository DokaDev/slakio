//! A pane: one conversation or thread, its loaded messages, the selection, a VISUAL range and
//! its own composer. Messages arrive in pages from the backend (newest first, then older ones
//! as the selection nears the top) and are sanitised once, on arrival, into [`Shown`] messages;
//! drawing them lays out only the rows on screen.
//!
//! The view is anchored at the bottom: `bottom` is the message on the last row, or the newest
//! while nothing is selected (so new messages stay in view). Drawing moves `bottom` to keep the
//! selection on screen; it is a `Cell` because only drawing knows the rows' heights. At the top
//! of a history longer than the screen (`gg`), the oldest message is on the first row and newer
//! ones below `bottom` fill what is left; a history that fits whole sits at the bottom.

use super::composer::Composer;
use slakio_core::backend::{Generation, Page};
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
    /// Oldest first.
    pub items: Vec<Shown>,
    /// The oldest message is loaded.
    pub complete: bool,
    /// A page was asked for and has not arrived; its generation.
    pub pending: Option<Generation>,
    /// The selected message (index into `items`); `None` until the user moves.
    pub selected: Option<usize>,
    /// The other end of the VISUAL range, while selecting one.
    pub visual: Option<usize>,
    /// The message on the last row (`None`: the newest). Kept by drawing.
    pub bottom: Cell<Option<usize>>,
    /// `gg` was pressed before the oldest message was loaded: keep loading, then select it.
    pub to_oldest: bool,
    pub composer: Composer,
    /// The rows drawn last, by drawing (only it knows the rows' heights).
    pub hits: RefCell<Vec<Hit>>,
}

impl Pane {
    pub fn new(target: Target) -> Self {
        Self {
            target,
            items: Vec::new(),
            complete: false,
            pending: None,
            selected: None,
            visual: None,
            bottom: Cell::new(None),
            to_oldest: false,
            composer: Composer::default(),
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

    /// The page to ask for next, if one is wanted: the newest first, then older ones when the
    /// selection nears the top. `gg` waiting for the oldest needs no case of its own: it keeps
    /// the oldest loaded message selected, which is near the top.
    pub fn wants(&self) -> Option<Option<Ts>> {
        if self.pending.is_some() || self.complete {
            return None;
        }
        let Some(first) = self.items.first() else { return Some(None) };
        let near_top = self.selected.is_some_and(|s| s < PREFETCH);
        near_top.then_some(Some(first.ts))
    }

    /// A page arrived (already sanitised): put it before what is loaded.
    pub fn add_page(&mut self, page: &Page, shown: Vec<Shown>) {
        self.pending = None;
        self.complete = page.complete;
        // Only what is older than the loaded messages (an echo or a late page never doubles).
        let first = self.items.first().map(|m| m.ts);
        let older: Vec<Shown> = shown.into_iter().filter(|m| first.is_none_or(|f| m.ts < f)).collect();
        let n = older.len();
        if n > 0 {
            self.items.splice(0..0, older);
            let shift = |i: Option<usize>| i.map(|i| i + n);
            self.selected = shift(self.selected);
            self.visual = shift(self.visual);
            self.bottom.set(shift(self.bottom.get()));
        }
        if self.to_oldest {
            self.selected = (!self.items.is_empty()).then_some(0);
            self.to_oldest = !self.complete;
        }
    }

    /// Select the next (`1`) or previous (`-1`) message; the first move selects the newest.
    pub fn step(&mut self, by: isize) {
        self.to_oldest = false;
        let Some(last) = self.items.len().checked_sub(1) else { return };
        self.selected = Some(match self.selected {
            None => last,
            Some(s) => s.saturating_add_signed(by).min(last),
        });
    }

    pub fn select_newest(&mut self) {
        self.to_oldest = false;
        self.selected = self.items.len().checked_sub(1);
    }

    /// The message this thread pane is the thread of, when loaded (index into `items`).
    pub fn root(&self) -> Option<usize> {
        let Target::Thread { thread, .. } = &self.target else { return None };
        self.items.iter().position(|m| m.ts == *thread)
    }

    /// Select the message on screen row `y` as last drawn; `Some(link)` when there is one.
    pub fn hit(&self, y: u16) -> Option<Hit> {
        self.hits.borrow().iter().find(|h| h.y == y).copied()
    }

    /// Select the oldest message, loading the rest of the history first.
    pub fn select_oldest(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = Some(0);
        self.to_oldest = !self.complete;
    }

    /// The selected messages: the VISUAL range, or the selected one.
    pub fn range(&self) -> Option<(usize, usize)> {
        let s = self.selected?;
        let v = self.visual.unwrap_or(s);
        Some((s.min(v), s.max(v)))
    }

    /// Add the user's own message (the demo's local echo), after the newest.
    pub fn echo(&mut self, user: UserId, author: Safe, text: Safe) {
        let ts = self.items.last().map_or(Ts(0), |m| Ts(m.ts.0 + 1_000_000));
        self.items.push(Shown {
            ts,
            user,
            author,
            own: true,
            text,
            thread: None,
            reactions: Vec::new(),
            edited: false,
        });
        // Back to the newest, so the message just written is in view.
        self.selected = None;
        self.visual = None;
        self.bottom.set(None);
    }
}

#[cfg(test)]
mod tests;
