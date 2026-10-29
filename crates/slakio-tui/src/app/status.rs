//! The status line's message slot: one transient notice at a time. A warning is not replaced by
//! a weaker (info) notice while it shows, so the reason something did not work stays readable.
//!
//! A notice that must be seen (the start's notices about the cache) is queued instead: it shows
//! once the slot is free, and when another notice takes the slot from it, it shows again after
//! that one, so each is seen at least once whatever comes at the same time.

use slakio_core::i18n::Msg;
use slakio_core::model::WorkspaceId;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How long an info notice shows.
pub const INFO_FOR: Duration = Duration::from_secs(3);
/// How long a warning shows.
pub const WARNING_FOR: Duration = Duration::from_secs(6);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Info,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub msg: Msg,
    pub level: Level,
    pub until: Instant,
    /// The workspace it is about (its stripe shows beside it), when it is about one.
    pub about: Option<WorkspaceId>,
}

/// The message slot.
#[derive(Clone, Debug, Default)]
pub struct Status {
    notice: Option<Notice>,
    /// The notice showing was queued: taken from the slot early, it is queued again.
    queued: bool,
    /// Notices that must be seen, waiting for the slot, in order.
    waiting: VecDeque<(Msg, Level, Option<WorkspaceId>)>,
}

impl Status {
    /// The notice showing at `now`, if any.
    pub fn notice(&self, now: Instant) -> Option<&Notice> {
        self.notice.as_ref().filter(|n| n.until > now)
    }

    /// Show `msg` from `now` on, unless a stronger notice is still showing.
    pub fn show(&mut self, msg: impl Into<Msg>, level: Level, now: Instant) {
        self.show_about(msg, level, None, now);
    }

    /// [`Status::show`] for a notice about workspace `about`.
    pub fn show_about(&mut self, msg: impl Into<Msg>, level: Level, about: Option<WorkspaceId>, now: Instant) {
        if self.notice(now).is_some_and(|n| n.level > level) {
            return;
        }
        if self.queued
            && let Some(n) = self.notice.take().filter(|n| n.until > now)
        {
            self.waiting.push_front((n.msg, n.level, n.about));
        }
        self.put(msg.into(), level, about, now, false);
    }

    /// Show `msg` from `now` on when nothing shows, else as soon as what shows (and what was
    /// queued before it) is done: it is seen at least once.
    pub fn queue(&mut self, msg: impl Into<Msg>, level: Level, now: Instant) {
        self.queue_about(msg, level, None, now);
    }

    /// [`Status::queue`] for a notice about workspace `about`.
    pub fn queue_about(&mut self, msg: impl Into<Msg>, level: Level, about: Option<WorkspaceId>, now: Instant) {
        if self.notice(now).is_none() && self.waiting.is_empty() {
            self.put(msg.into(), level, about, now, true);
        } else {
            self.waiting.push_back((msg.into(), level, about));
        }
    }

    /// Say `msg` in place of what `stale` says is no longer true: those notices waiting are
    /// forgotten, and the one showing too, so `msg` shows at once in its place. Otherwise a
    /// warning shows as [`Status::show_about`] says, and must be seen (when another notice takes
    /// the slot from it, it shows again after that one, as a queued one); anything else is queued.
    pub fn restate_about(
        &mut self,
        msg: impl Into<Msg>,
        level: Level,
        about: Option<WorkspaceId>,
        now: Instant,
        stale: impl Fn(&Msg) -> bool,
    ) {
        self.forget(now, stale);
        match level {
            Level::Warning => {
                let msg = msg.into();
                self.show_about(msg.clone(), level, about, now);
                if self.notice(now).is_some_and(|n| n.msg == msg) {
                    self.queued = true;
                }
            }
            Level::Info => self.queue_about(msg, level, about, now),
        }
    }

    /// Forget the notices `stale` says are no longer true at `now`: those waiting, and the one
    /// showing, whose slot the next one waiting takes.
    pub fn forget(&mut self, now: Instant, stale: impl Fn(&Msg) -> bool) {
        self.waiting.retain(|(m, _, _)| !stale(m));
        if self.notice(now).is_some_and(|n| stale(&n.msg)) {
            self.notice = None;
            if let Some((msg, level, about)) = self.waiting.pop_front() {
                self.put(msg, level, about, now, true);
            }
        }
    }

    /// A notice `stale` matches waits for the slot, and none shows: it was not seen (yet, or
    /// again after another took the slot from it).
    pub fn unseen(&self, now: Instant, stale: impl Fn(&Msg) -> bool) -> bool {
        self.waiting.iter().any(|(m, _, _)| stale(m)) && !self.notice(now).is_some_and(|n| stale(&n.msg))
    }

    fn put(&mut self, msg: Msg, level: Level, about: Option<WorkspaceId>, now: Instant, queued: bool) {
        let until = now + if level == Level::Warning { WARNING_FOR } else { INFO_FOR };
        self.notice = Some(Notice { msg, level, until, about });
        self.queued = queued;
    }

    /// When the screen changes next by itself (the notice goes away), if ever.
    pub fn deadline(&self) -> Option<Instant> {
        self.notice.as_ref().map(|n| n.until)
    }

    /// Forget a notice whose time is up at `now`, and show the next one waiting. `true` when
    /// one went (the screen changed).
    pub fn expire(&mut self, now: Instant) -> bool {
        if self.notice.as_ref().is_some_and(|n| n.until <= now) {
            self.notice = None;
            if let Some((msg, level, about)) = self.waiting.pop_front() {
                self.put(msg, level, about, now, true);
            }
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests;
