//! The rails every request passes, for the whole process (every workspace together):
//!
//! * the **kill switch** — with the network off nothing is sent, and switching it off stops
//!   the requests in flight too ([`Governor::stop`], `SLAKIO_NETWORK=off`, `network = "off"`);
//! * at most [`Limits::in_flight`] requests in flight, and at most [`Limits::waiting`] waiting
//!   for a slot (more are refused as busy);
//! * a **request cap** per minute, with a share that background work cannot use, so what the
//!   user asked for goes first; and a cap for the life of the process that, once reached,
//!   stops all network until the program is started again.
//!
//! The defaults are far above what the client needs (its start is three requests, opening a
//! conversation one): they are ceilings that stop a bug from flooding Slack, not a pace.

use crate::error::SlackError;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

/// The environment variable that switches the network off for the whole process: `off`, or
/// `on` (the default; the config file's `network` decides).
pub const NETWORK_ENV: &str = "SLAKIO_NETWORK";

/// Whether the network may be used, from [`NETWORK_ENV`] (`env`) and the config's `network`
/// (`config`, already checked). Either one `off` wins; a value of the variable that is not
/// understood is an error (the binary exits): a typo must not leave the network on.
pub fn network_on(env: Option<&str>, config: &str) -> Result<bool, String> {
    match env.map(str::to_ascii_lowercase).as_deref() {
        None | Some("" | "on") => Ok(config != "off"),
        Some("off") => Ok(false),
        Some(other) => Err(format!("{NETWORK_ENV}={other} is not understood (on or off)")),
    }
}

/// The window of the per-minute cap.
pub const WINDOW: Duration = Duration::from_secs(60);

/// How much may be sent, and how much at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Requests in any [`WINDOW`], all priorities together.
    pub per_minute: u32,
    /// Of those, the most background work may use: the rest is kept for the user.
    pub background_per_minute: u32,
    /// Requests for the life of the process; reaching it stops all network.
    pub per_process: u32,
    /// Requests in flight at once.
    pub in_flight: usize,
    /// Requests waiting for a slot; more are refused as busy.
    pub waiting: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { per_minute: 120, background_per_minute: 90, per_process: 5000, in_flight: 8, waiting: 64 }
    }
}

/// Who a request is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    /// Something the user is waiting for (the start, the conversation they opened).
    User,
    /// Work nobody waits for (counts, lookups): it yields when the window is near full.
    Background,
}

/// A monotonic clock, so the tests can move time.
pub trait Clock: Send + Sync {
    /// Time since an arbitrary start.
    fn now(&self) -> Duration;
}

/// The real clock.
#[derive(Debug)]
pub struct SystemClock(Instant);

impl Default for SystemClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

/// A clock that moves only when told to (tests).
#[derive(Clone, Debug, Default)]
pub struct FakeClock(Arc<AtomicU64>);

impl FakeClock {
    pub fn advance(&self, by: Duration) {
        self.0.fetch_add(u64::try_from(by.as_millis()).unwrap_or(u64::MAX), Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::SeqCst))
    }
}

#[derive(Debug, Default)]
struct Counts {
    /// When each request of the current window was let through, oldest first.
    window: VecDeque<Duration>,
    total: u32,
    /// The per-process cap was reached: nothing more is sent.
    tripped: bool,
}

/// The process's rails. One is shared by every workspace's transport.
pub struct Governor {
    limits: Limits,
    clock: Box<dyn Clock>,
    counts: Mutex<Counts>,
    network: watch::Sender<bool>,
    slots: Arc<Semaphore>,
    waiting: AtomicUsize,
}

impl std::fmt::Debug for Governor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Governor").field("limits", &self.limits).field("network", &self.is_on()).finish_non_exhaustive()
    }
}

/// A request let through: it holds a slot until dropped.
#[derive(Debug)]
pub struct Admitted {
    _slot: OwnedSemaphorePermit,
}

impl Governor {
    /// `network`: whether the network may be used at all.
    pub fn new(limits: Limits, clock: Box<dyn Clock>, network: bool) -> Arc<Self> {
        Arc::new(Self {
            limits,
            clock,
            counts: Mutex::new(Counts::default()),
            network: watch::Sender::new(network),
            slots: Arc::new(Semaphore::new(limits.in_flight)),
            waiting: AtomicUsize::new(0),
        })
    }

    /// The kill switch: nothing more is sent, and what is in flight stops ([`Governor::stopped`]).
    pub fn stop(&self) {
        self.network.send_replace(false);
    }

    pub fn is_on(&self) -> bool {
        *self.network.borrow()
    }

    /// Done when the network is switched off (at once when it is off).
    pub async fn stopped(&self) {
        let mut rx = self.network.subscribe();
        // The sender lives as long as `self`, so the wait ends only when the switch is off.
        let _ = rx.wait_for(|on| !on).await;
    }

    /// Let a request of `priority` through, waiting for a slot when all are taken: nothing may
    /// be sent without it.
    pub async fn admit(&self, priority: Priority) -> Result<Admitted, SlackError> {
        let off = || SlackError::Offline { why: "the network is switched off" };
        if !self.is_on() {
            return Err(off());
        }
        if self.waiting.fetch_add(1, Ordering::SeqCst) >= self.limits.waiting {
            self.waiting.fetch_sub(1, Ordering::SeqCst);
            return Err(SlackError::Busy);
        }
        let slot = tokio::select! {
            slot = Arc::clone(&self.slots).acquire_owned() => slot,
            () = self.stopped() => {
                self.waiting.fetch_sub(1, Ordering::SeqCst);
                return Err(off());
            }
        };
        self.waiting.fetch_sub(1, Ordering::SeqCst);
        let Ok(slot) = slot else { return Err(off()) };
        if !self.is_on() {
            return Err(off());
        }
        self.count(priority)?;
        Ok(Admitted { _slot: slot })
    }

    /// Count a request against the caps, or refuse it.
    fn count(&self, priority: Priority) -> Result<(), SlackError> {
        let now = self.clock.now();
        let mut c = self.counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if c.tripped {
            return Err(SlackError::CapReached { why: "the request cap of this run is reached" });
        }
        while c.window.front().is_some_and(|at| now.saturating_sub(*at) >= WINDOW) {
            c.window.pop_front();
        }
        // Background work stops once the window holds its share, whoever sent those: the rest
        // of the window stays free for the user.
        let used = u32::try_from(c.window.len()).unwrap_or(u32::MAX);
        let over = match priority {
            Priority::User => used >= self.limits.per_minute,
            Priority::Background => used >= self.limits.background_per_minute.min(self.limits.per_minute),
        };
        if over {
            return Err(SlackError::CapReached { why: "the per-minute request cap is reached" });
        }
        if c.total >= self.limits.per_process {
            c.tripped = true;
            drop(c);
            self.stop();
            return Err(SlackError::CapReached { why: "the request cap of this run is reached" });
        }
        c.total += 1;
        c.window.push_back(now);
        Ok(())
    }

    /// Requests let through so far.
    pub fn sent(&self) -> u32 {
        self.counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).total
    }
}

#[cfg(test)]
mod tests;
