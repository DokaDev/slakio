//! Photos asked for ahead: the photo of every 1:1 DM's peer the list lists, the rows scrolled
//! out of view too, so a row scrolled into view draws its photo at once. It is the pictures'
//! lowest priority and never in the way of what is on screen:
//! - it goes only once every picture the frame wanted is asked for or looked for, at most
//!   [`AHEAD_MAX`] at once, and asks the network only for an idle one of the
//!   [`IN_FLIGHT_MAX`] slots (so the network never has more than those, which is what the
//!   backend lets through before it answers busy);
//! - each request goes through the same request budget as any picture (the governor's share
//!   for pictures), and at most [`AHEAD_PER_MINUTE`] a minute and [`AHEAD_PER_HOUR`] an hour of
//!   them are asked ahead. The governor counts every hop of a photo (up to three, with its
//!   redirects): at worst 12 of a workspace's 30 pictures a minute (120 of 350 an hour) go
//!   ahead, so what is on screen keeps at least 18, and the rest of the budget stays with
//!   everything else;
//! - an answer that the backend is busy or offline, or that the budget is spent, for any
//!   picture, stops it for [`RETRY_AFTER`]; one that Slack asks to slow down (429), for as long
//!   as it says and at least that; a pause only ever lasts longer;
//! - it needs a cache on disk ([`super::cached`]): a photo is looked for there first and asked
//!   of the network only when it is not there, and every photo fetched is kept there, so a row
//!   drawn later reads it at once. Without one (and in the demo) nothing is asked ahead.
//!
//! A photo that comes while its row is on screen is drawn as any other. One whose row is not
//! goes to the terminal only when it fits beside what the terminal holds and what is on its way,
//! removing nothing; otherwise it is let go (the disk has it). Each is asked ahead once a run.
//! Pictures or photos turned off, nothing waits to be asked ahead.

use super::{BYTES_CAP, CHIP_PHOTO, ENTRIES_MAX, Entry, IN_FLIGHT_MAX, Images, Key, RETRY_AFTER, note};
use crate::app::requests::Requests;
use slakio_core::backend::{Command, FailKind};
use slakio_core::diag::Stage;
use slakio_core::images::{BYTES_MAX, Source};
use slakio_core::model::WorkspaceId;
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

/// The most photos asked for ahead (or looked for on disk) at once.
pub const AHEAD_MAX: usize = 1;
/// The most photos asked of the network ahead in any minute (each up to three of the
/// governor's picture counts, with its redirects).
pub const AHEAD_PER_MINUTE: usize = 4;
/// The most photos asked of the network ahead in any hour.
pub const AHEAD_PER_HOUR: usize = 40;
/// The most photos waiting to be asked for ahead.
pub const AHEAD_QUEUE_MAX: usize = 128;

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(3600);

/// The photos asked for ahead: those waiting, those on their way, those done this run.
#[derive(Debug, Default)]
pub(super) struct Ahead {
    queue: VecDeque<(WorkspaceId, Key)>,
    pub(super) flying: HashSet<Key>,
    done: HashSet<Key>,
    /// When each of the last hour's requests ahead was asked, the oldest first.
    sent: VecDeque<Instant>,
    /// Nothing is asked of the network ahead before then.
    pub(super) paused_until: Option<Instant>,
}

impl Images {
    /// The photos of the DM peers the list lists (`dms`, top down) are to be asked for ahead,
    /// when pictures and `photos` are on, with a cache on disk and not in the demo; else none is.
    pub fn offer_ahead(&mut self, photos: bool, dms: impl IntoIterator<Item = (WorkspaceId, Source)>) {
        self.ahead.queue.clear();
        if !(self.on && photos && !self.demo && self.cache.is_some()) {
            return;
        }
        let mut queued = HashSet::new();
        for (ws, source) in dms {
            if self.ahead.queue.len() >= AHEAD_QUEUE_MAX {
                break;
            }
            let key = (source, CHIP_PHOTO);
            if self.entries.contains_key(&key) || self.ahead.done.contains(&key) || !queued.insert(key.clone()) {
                continue;
            }
            self.ahead.queue.push_back((ws, key));
        }
    }

    /// The photos waiting to be asked for ahead.
    pub fn ahead_waiting(&self) -> usize {
        self.ahead.queue.len()
    }

    /// Nothing more is asked for ahead, nor waited for ([`Images::resolve`], photos off); what
    /// was asked ahead still counts toward the minute's and the hour's share, and a pause stays.
    pub(super) fn forget_ahead(&mut self) {
        let Ahead { sent, paused_until, .. } = std::mem::take(&mut self.ahead);
        self.ahead = Ahead { sent, paused_until, ..Ahead::default() };
    }

    /// A picture failed for `kind` at `now`: busy, offline or the budget spent, nothing is asked
    /// of the network ahead for [`super::RETRY_AFTER`]; asked to slow down, for as long as it
    /// said and at least that. A pause only ever lasts longer, never shorter.
    pub(super) fn pause_ahead(&mut self, kind: &FailKind, now: Instant) {
        let pause = match kind {
            FailKind::Busy | FailKind::Offline | FailKind::CapReached => RETRY_AFTER,
            FailKind::RateLimited { retry_after } => retry_after.unwrap_or(RETRY_AFTER).max(RETRY_AFTER),
            _ => return,
        };
        let until = now + pause;
        self.ahead.paused_until = Some(self.ahead.paused_until.map_or(until, |t| t.max(until)));
    }

    /// The photos asked for ahead still on their way.
    fn flying(&mut self) -> usize {
        let entries = &self.entries;
        let on_way = |k: &Key| matches!(entries.get(k), Some(Entry::Asked(_) | Entry::Decoding(_) | Entry::Looking(_)));
        self.ahead.flying.retain(on_way);
        self.ahead.flying.len()
    }

    /// The network may be asked ahead at `now`: an idle slot, no pause, within the minute's and
    /// the hour's share.
    fn may_fetch(&mut self, now: Instant) -> bool {
        while self.ahead.sent.front().is_some_and(|t| now.saturating_duration_since(*t) >= HOUR) {
            self.ahead.sent.pop_front();
        }
        let minute = self.ahead.sent.iter().filter(|t| now.saturating_duration_since(**t) < MINUTE).count();
        self.in_flight() < IN_FLIGHT_MAX
            && self.ahead.paused_until.is_none_or(|t| now >= t)
            && minute < AHEAD_PER_MINUTE
            && self.ahead.sent.len() < AHEAD_PER_HOUR
    }

    /// Once everything the frame wanted is asked for: the next photo waiting is looked for on
    /// disk, or (missed there) asked of the network when it may be. `true` when one was.
    pub(super) fn ask_ahead(&mut self, requests: &mut Requests, now: Instant) -> bool {
        if !self.on || self.cache.is_none() || self.flying() >= AHEAD_MAX {
            return false;
        }
        while let Some((ws, key)) = self.ahead.queue.pop_front() {
            if self.entries.contains_key(&key) || self.ahead.done.contains(&key) {
                continue;
            }
            if !self.missed.contains(&key) {
                self.look_up(key.clone());
                self.ahead.flying.insert(key);
                return true;
            }
            if !self.may_fetch(now) {
                self.ahead.queue.push_front((ws, key));
                return false;
            }
            self.missed.remove(&key);
            note::line(&self.diag, &key, Stage::Asked, "ahead", &[]);
            let id = requests.ask(Some(ws), Command::Image { source: key.0.clone(), max: BYTES_MAX });
            self.asked.insert(id, key.clone());
            self.entries.insert(key.clone(), Entry::Asked(id));
            self.ahead.sent.push_back(now);
            self.ahead.done.insert(key.clone());
            self.ahead.flying.insert(key);
            return true;
        }
        false
    }

    /// A photo asked for ahead came, `bytes` large: `true` when it is let go, not sent to the
    /// terminal (its row is not on screen and it does not fit beside what is held and what is on
    /// its way, removing nothing).
    pub(super) fn let_go_ahead(&mut self, key: &Key, bytes: usize) -> bool {
        self.ahead.done.insert(key.clone());
        let drawn = self.drawn.borrow().iter().any(|d| d.1 && d.0 == *key);
        if drawn || self.on_screen(key) {
            return false;
        }
        // What else is on its way: the network's and the disk's, not this one.
        let mine = usize::from(matches!(self.entries.get(key), Some(Entry::Decoding(_) | Entry::Looking(_))));
        let others = (self.in_flight() + self.looking()).saturating_sub(mine);
        if self.held.len() + others < ENTRIES_MAX && self.bytes + bytes <= BYTES_CAP {
            return false;
        }
        note::line(&self.diag, key, Stage::Skipped, "ahead-room", &[]);
        self.entries.remove(key);
        true
    }
}

#[cfg(test)]
mod tests;
