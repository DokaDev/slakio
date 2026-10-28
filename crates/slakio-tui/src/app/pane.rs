//! A pane: a view of one conversation or thread — the selection, a VISUAL range, Insert mode,
//! where the view is anchored, the rows drawn last, the thread panel it opened and what it showed
//! before and after (its back/forward [`History`]). The messages are not the pane's: they are the
//! target's [`Timeline`], shared by every pane that shows it, and what is being written is the
//! target's draft ([`super::drafts`]). A pane asks for one more page of older messages when a
//! gesture takes its selection near the top (moving up, `gg`), never by itself: a page that
//! arrives asks for nothing more. Drawing lays out only the rows on screen.
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
use crate::mrkdwn::Block;
use slakio_core::layout::PaneId;
use slakio_core::model::{Message, Target, ThreadSummary, Ts, UserId};
use slakio_core::sanitize::Safe;
use std::cell::{Cell, RefCell};

/// Targets a pane's back history keeps.
pub const HISTORY: usize = 50;

/// What a pane showed before (`Ctrl+O`) and after (`Ctrl+I`) what it shows now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    pub back: Vec<Target>,
    pub forward: Vec<Target>,
}

impl History {
    /// `from` was left for something new: it is the last one back, and nothing is forward.
    pub fn left(&mut self, from: Target) {
        self.back.push(from);
        if self.back.len() > HISTORY {
            self.back.remove(0);
        }
        self.forward.clear();
    }

    /// Back from `from`: the target to show, if any (`from` goes forward).
    pub fn back(&mut self, from: &Target) -> Option<Target> {
        let to = self.back.pop()?;
        self.forward.push(from.clone());
        Some(to)
    }

    /// Forward from `from`: the target to show, if any (`from` goes back).
    pub fn forward(&mut self, from: &Target) -> Option<Target> {
        let to = self.forward.pop()?;
        self.back.push(from.clone());
        Some(to)
    }

    /// Keep only the targets `keep` says still exist.
    pub fn retain(&mut self, keep: impl Fn(&Target) -> bool) {
        self.back.retain(&keep);
        self.forward.retain(&keep);
    }
}

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
    /// The text read as Slack's markup, once, when the message arrives.
    pub body: Vec<Block>,
    pub thread: Option<ThreadSummary>,
    pub reactions: Vec<ShownReaction>,
    pub edited: bool,
}

impl Shown {
    /// `m` as drawn, by `author` (the sender's name, sanitised), `own` when the user wrote it.
    pub fn new(m: &Message, author: Safe, own: bool) -> Self {
        let text = m.text.block();
        Self {
            ts: m.ts,
            user: m.user.clone(),
            author,
            own,
            body: crate::mrkdwn::parse(text.as_str()),
            text,
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
    /// The last gesture moved up (or to the oldest): near the top, it wants one more older
    /// page. Asking for pages takes it back.
    pub older: bool,
    /// `gg` in a thread: its older pages are asked for one after another, and its own message
    /// is selected once loaded (they come from what the backend read of it, at once). Any other
    /// move takes it back.
    pub to_root: bool,
    /// Its composer is being written in (Insert mode); only the pane with the keyboard is.
    pub insert: bool,
    /// The auto thread panel this pane opened beside it, while open.
    pub thread: Option<PaneId>,
    /// What it showed before and after.
    pub history: History,
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
            older: false,
            to_root: false,
            insert: false,
            thread: None,
            history: History::default(),
            hits: RefCell::new(Vec::new()),
        }
    }

    /// Show `target` instead, from scratch: no selection, the newest in view. Its history stays
    /// (the caller says where `target` came from).
    pub fn show(&mut self, target: Target) {
        let history = std::mem::take(&mut self.history);
        *self = Self { history, ..Self::new(target) };
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
    /// first, then an older one when the last gesture moved up near the top (also one that
    /// failed, when it may be asked for again), or `gg` in a thread wants its start.
    pub fn wants(&self, tl: &Timeline) -> Option<Option<Ts>> {
        let older = self.older || self.to_root;
        let again = older && tl.older_failed().is_some() && tl.retryable();
        if tl.pending.is_some() || tl.queued || tl.complete || (tl.failed.is_some() && !again) {
            return None;
        }
        if tl.cached {
            return Some(None);
        }
        let Some(first) = tl.items.first() else { return Some(None) };
        let near_top = self.selected_index(tl).is_some_and(|s| s < PREFETCH);
        (self.to_root || (self.older && near_top)).then_some(Some(first.ts))
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
        self.older = by < 0;
        self.to_root = false;
        let Some(last) = tl.items.len().checked_sub(1) else { return };
        let i = match self.selected_index(tl) {
            None => last,
            Some(s) => s.saturating_add_signed(by).min(last),
        };
        self.select_index(i, tl);
    }

    /// Select the newest message of `tl` (this pane's timeline).
    pub fn select_newest(&mut self, tl: &Timeline) {
        self.to_root = false;
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

    /// Select the oldest message loaded in `tl` (its timeline); one more older page is wanted
    /// (in a thread, every one up to its own message).
    pub fn select_oldest(&mut self, tl: &Timeline) {
        if tl.items.is_empty() {
            return;
        }
        self.select_index(0, tl);
        self.older = true;
        self.to_root = self.is_thread() && !tl.complete;
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
    let body = crate::mrkdwn::parse(text.as_str());
    items.push(Shown { ts, user, author, own: true, text, body, thread: None, reactions: Vec::new(), edited: false });
}

#[cfg(test)]
pub(crate) mod tests;
