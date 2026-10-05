//! The status line's message slot: one transient notice at a time. A warning is not replaced by
//! a weaker (info) notice while it shows, so the reason something did not work stays readable.

use slakio_core::i18n::Msg;
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
}

/// The message slot.
#[derive(Clone, Debug, Default)]
pub struct Status {
    notice: Option<Notice>,
}

impl Status {
    /// The notice showing at `now`, if any.
    pub fn notice(&self, now: Instant) -> Option<&Notice> {
        self.notice.as_ref().filter(|n| n.until > now)
    }

    /// Show `msg` from `now` on, unless a stronger notice is still showing.
    pub fn show(&mut self, msg: impl Into<Msg>, level: Level, now: Instant) {
        if self.notice(now).is_some_and(|n| n.level > level) {
            return;
        }
        let until = now + if level == Level::Warning { WARNING_FOR } else { INFO_FOR };
        self.notice = Some(Notice { msg: msg.into(), level, until });
    }

    /// When the screen changes next by itself (the notice goes away), if ever.
    pub fn deadline(&self) -> Option<Instant> {
        self.notice.as_ref().map(|n| n.until)
    }

    /// Forget a notice whose time is up at `now`. `true` when one went (the screen changed).
    pub fn expire(&mut self, now: Instant) -> bool {
        if self.notice.as_ref().is_some_and(|n| n.until <= now) {
            self.notice = None;
            return true;
        }
        false
    }
}
