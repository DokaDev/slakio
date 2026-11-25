//! The picture hosts' own budget: profile photos (the avatar hosts) and a workspace's own emoji
//! (the emoji host) are asked of a CDN with nothing of the session (no cookie, no token), so they
//! never count against Slack's API: not in the minute's cap, the hour's budget, a lane's
//! background share, the pictures' share nor the API's run total. They keep to a budget of
//! their own instead, for the whole process:
//!
//! * at most [`CdnLimits::per_minute`] in any minute and [`CdnLimits::per_hour`] in any hour,
//!   the list's photos (a DM row's, one asked ahead) at most two thirds of each, so the
//!   timeline's always have the rest;
//! * at most [`CdnLimits::in_flight`] on their way at once ([`Governor::cdn_slot`]);
//! * a **pause per host** when one answers 429: that host is not asked again for its
//!   `Retry-After` ([`HOST_PAUSE`] without one, at most [`HOST_PAUSE_MAX`]); the others go on;
//! * a **run alarm** of its own, [`CdnLimits::per_run`]: once reached, no picture host is asked
//!   again this run (photos and emoji stay initials and text), while the API, the socket and
//!   everything else go on as before. It never switches the network off.
//!
//! The kill switch holds here as everywhere: with the network off nothing is asked.

use super::{Governor, HOUR, list_part, over_share};
use crate::error::SlackError;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// How long a picture host is not asked after a 429 that did not say for how long.
pub const HOST_PAUSE: Duration = Duration::from_secs(30);
/// The longest a picture host is paused, whatever its answer said.
pub const HOST_PAUSE_MAX: Duration = Duration::from_secs(3600);
/// Why a picture is refused once the picture hosts' run alarm went off.
pub const CDN_RUN_REACHED: &str = "the picture hosts' run limit is reached";

/// How much may be asked of the picture hosts, and how much at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CdnLimits {
    /// Asks in any [`super::WINDOW`], every host together.
    pub per_minute: u32,
    /// Asks in any [`HOUR`], every host together.
    pub per_hour: u32,
    /// Asks on their way at once.
    pub in_flight: usize,
    /// Asks for the life of the process: past it, no picture host is asked again this run.
    pub per_run: u64,
}

impl Default for CdnLimits {
    fn default() -> Self {
        Self { per_minute: 300, per_hour: 3000, in_flight: 12, per_run: 20_000 }
    }
}

/// What the picture hosts were asked.
#[derive(Debug, Default)]
pub(super) struct CdnCounts {
    /// When each ask of the last hour was let through, oldest first, and whether it was the list's.
    times: VecDeque<(Duration, bool)>,
    /// Hosts that answered 429, and until when they are not asked.
    paused: HashMap<String, Duration>,
    /// Asks let through this run.
    total: u64,
    /// The run alarm was told about already.
    told: bool,
}

/// A place among the picture hosts' asks on their way: given back when dropped.
#[derive(Debug)]
pub struct CdnSlot {
    _slot: OwnedSemaphorePermit,
}

impl Governor {
    /// [`Governor::new`], with `cdn` for the picture hosts' budget.
    pub fn with_cdn(limits: super::Limits, cdn: CdnLimits, clock: Box<dyn super::Clock>, network: bool) -> Arc<Self> {
        Arc::new(Self {
            limits,
            clock,
            counts: std::sync::Mutex::default(),
            network: tokio::sync::watch::Sender::new(network),
            locked: !network,
            slots: Arc::new(Semaphore::new(limits.in_flight)),
            waiting: std::sync::atomic::AtomicUsize::new(0),
            lanes: std::sync::Mutex::default(),
            cdn_limits: cdn,
            cdn_slots: Arc::new(Semaphore::new(cdn.in_flight.max(1))),
        })
    }

    /// A photo or an emoji may be asked of `host` now (`list`: for the list): the network is on,
    /// the run alarm has not gone off, `host` is not paused, and the picture hosts' budget has
    /// room. Counted against that budget only, never against the API's.
    pub fn cdn(&self, host: &str, list: bool) -> Result<(), SlackError> {
        if !self.is_on() {
            return Err(SlackError::Offline { why: "the network is switched off" });
        }
        let now = self.clock.now();
        let mut c = self.counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let cdn = &mut c.cdn;
        if cdn.total >= self.cdn_limits.per_run {
            return Err(SlackError::CapReached { why: CDN_RUN_REACHED });
        }
        if let Some(until) = cdn.paused.get(host).filter(|until| **until > now) {
            return Err(SlackError::RateLimited { retry_after: Some(*until - now) });
        }
        while cdn.times.front().is_some_and(|(at, _)| now.saturating_sub(*at) >= HOUR) {
            cdn.times.pop_front();
        }
        let (minute, hour) = (self.cdn_limits.per_minute, self.cdn_limits.per_hour);
        if over_share(cdn.times.iter().map(|t| t.0), now, minute, hour) {
            return Err(SlackError::CapReached { why: "the picture hosts' budget is reached" });
        }
        let lists = cdn.times.iter().filter(|t| t.1).map(|t| t.0);
        if list && over_share(lists, now, list_part(minute), list_part(hour)) {
            return Err(SlackError::CapReached { why: "the list's part of the picture hosts' budget is reached" });
        }
        cdn.total += 1;
        cdn.times.push_back((now, list));
        Ok(())
    }

    /// `true` the first time it is asked after the run alarm went off ([`CDN_RUN_REACHED`]):
    /// what is told once only.
    pub fn cdn_alarm_first(&self) -> bool {
        let mut c = self.counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let first = c.cdn.total >= self.cdn_limits.per_run && !c.cdn.told;
        c.cdn.told |= first;
        first
    }

    /// A place among the picture hosts' asks on their way ([`CdnLimits::in_flight`]), waited
    /// for; switching the network off ends the wait.
    pub async fn cdn_slot(&self) -> Result<CdnSlot, SlackError> {
        let off = || SlackError::Offline { why: "the network is switched off" };
        if !self.is_on() {
            return Err(off());
        }
        let slot = tokio::select! {
            slot = Arc::clone(&self.cdn_slots).acquire_owned() => slot,
            () = self.stopped() => return Err(off()),
        };
        slot.map(|_slot| CdnSlot { _slot }).map_err(|_| off())
    }

    /// `host` answered 429: it is not asked again for `retry_after` ([`HOST_PAUSE`] when it did
    /// not say), at least a second and at most [`HOST_PAUSE_MAX`]. A pause only ever lasts longer.
    pub fn pause_host(&self, host: &str, retry_after: Option<Duration>) {
        let pause = retry_after.unwrap_or(HOST_PAUSE).clamp(Duration::from_secs(1), HOST_PAUSE_MAX);
        let until = self.clock.now() + pause;
        let mut c = self.counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let at = c.cdn.paused.entry(host.to_string()).or_default();
        *at = (*at).max(until);
    }

    /// How long `host` is still paused, if it is.
    pub fn host_paused(&self, host: &str) -> Option<Duration> {
        let now = self.clock.now();
        let c = self.counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        c.cdn.paused.get(host).filter(|until| **until > now).map(|until| *until - now)
    }
}

#[cfg(test)]
mod tests;
