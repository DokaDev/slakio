//! The rails every request passes, for the whole process (every workspace together):
//!
//! * the **kill switch** — with the network off nothing is sent, and switching it off stops
//!   the requests in flight too ([`Governor::stop`], `SLAKIO_NETWORK=off`, `network = "off"`);
//!   the app's `:net` switches it off and on again ([`Governor::switch`]), never on when it was
//!   off from the start or the run's cap was reached;
//! * at most [`Limits::in_flight`] requests in flight, and at most [`Limits::waiting`] waiting
//!   for a slot (more are refused as busy);
//! * a **request cap** per minute, with a share that background work cannot use, so what the
//!   user asked for goes first; a budget per hour, with such a share too; and a ceiling for the
//!   life of the process that, once reached, stops all network until the program is started
//!   again;
//! * **fair between workspaces** — each workspace's client has a [`Lane`] of its own: one of
//!   the slots is kept for it alone (for as many lanes as there are slots), and it holds at most
//!   [`Limits::lane_in_flight`] of the slots and [`Limits::lane_waiting`] of the waiting places,
//!   so busy workspaces, however many, never leave another without a slot; the caps and
//!   budgets are shared, but one workspace's background work (its socket's openings, catching
//!   up, lookups) takes at most [`Limits::lane_background_per_minute`] and
//!   [`Limits::lane_background_per_hour`] of them, so one that keeps reconnecting leaves the
//!   others room;
//! * a **pause per method** when Slack asks to slow down (HTTP 429 or `ratelimited`): that
//!   method is not sent again until its `Retry-After` has passed ([`DEFAULT_PAUSE`] without
//!   one); the others go on.
//!
//! The defaults are far above what the client needs (its start is three requests, opening a
//! conversation one): they are ceilings that stop a bug from flooding Slack, not a pace.

use crate::error::SlackError;
use crate::method::Method;
use std::collections::{HashMap, VecDeque};
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

/// The window of the hourly budget.
pub const HOUR: Duration = Duration::from_secs(3600);

/// How long a method waits after Slack asked to slow down without saying for how long.
pub const DEFAULT_PAUSE: Duration = Duration::from_secs(30);

/// How much may be sent, and how much at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Requests in any [`WINDOW`], all priorities together.
    pub per_minute: u32,
    /// Of those, the most background work may use: the rest is kept for the user.
    pub background_per_minute: u32,
    /// Requests in any [`HOUR`], all priorities together.
    pub per_hour: u32,
    /// Of those, the most background work may use: the rest of the hour is kept for the user.
    pub background_per_hour: u32,
    /// Of the background share of a minute, the most one workspace's lane uses.
    pub lane_background_per_minute: u32,
    /// Of the background share of an hour, the most one workspace's lane uses.
    pub lane_background_per_hour: u32,
    /// Requests for the life of the process: a safety ceiling far above any real use;
    /// reaching it stops all network until the program starts again.
    pub per_process: u32,
    /// Requests in flight at once.
    pub in_flight: usize,
    /// Requests waiting for a slot; more are refused as busy.
    pub waiting: usize,
    /// Of the slots, the most one workspace's lane holds at once.
    pub lane_in_flight: usize,
    /// Of the waiting places, the most one workspace's lane takes.
    pub lane_waiting: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            per_minute: 120,
            background_per_minute: 90,
            per_hour: 1500,
            background_per_hour: 1000,
            lane_background_per_minute: 60,
            lane_background_per_hour: 700,
            per_process: 10_000,
            in_flight: 8,
            waiting: 64,
            lane_in_flight: 6,
            lane_waiting: 48,
        }
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
    /// When each request of the last hour was let through, oldest first.
    window: VecDeque<Duration>,
    /// Methods Slack asked to slow down, and until when.
    paused: HashMap<Method, Duration>,
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
    /// The network was off from the start (`SLAKIO_NETWORK=off`, `--offline`, the config): it
    /// stays off for the run, whatever the app asks ([`Governor::switch`]).
    locked: bool,
    slots: Arc<Semaphore>,
    waiting: AtomicUsize,
}

impl std::fmt::Debug for Governor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Governor").field("limits", &self.limits).field("network", &self.is_on()).finish_non_exhaustive()
    }
}

/// A request let through: it holds a slot (and its lane's, when it came through one) until
/// dropped.
#[derive(Debug)]
pub struct Admitted {
    _slot: Option<OwnedSemaphorePermit>,
    _kept: Option<Kept>,
    _lane: Option<OwnedSemaphorePermit>,
}

/// One workspace's share of the rails ([`Governor::lane`]).
#[derive(Debug)]
pub struct Lane {
    slots: Arc<Semaphore>,
    waiting: AtomicUsize,
    limit: usize,
    /// The process's slot kept for this lane, while no request of it uses it (none when the
    /// lane was made with every slot taken).
    kept: Arc<Mutex<Option<OwnedSemaphorePermit>>>,
    /// When each of its background requests of the last hour was let through, oldest first.
    background: Mutex<VecDeque<Duration>>,
}

/// A lane's kept slot in use: it goes back to the lane when dropped.
#[derive(Debug)]
struct Kept {
    slot: Option<OwnedSemaphorePermit>,
    home: Arc<Mutex<Option<OwnedSemaphorePermit>>>,
}

impl Drop for Kept {
    fn drop(&mut self) {
        *self.home.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = self.slot.take();
    }
}

/// A waiting place taken: it is given back when dropped, also when the waiting request is
/// given up (its future dropped).
struct Waiting<'a>(&'a AtomicUsize);

impl<'a> Waiting<'a> {
    /// A place of `count`'s `limit`, or none when they are all taken.
    fn take(count: &'a AtomicUsize, limit: usize) -> Option<Self> {
        let place = Self(count);
        (count.fetch_add(1, Ordering::SeqCst) < limit).then_some(place)
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Governor {
    /// `network`: whether the network may be used at all.
    pub fn new(limits: Limits, clock: Box<dyn Clock>, network: bool) -> Arc<Self> {
        Arc::new(Self {
            limits,
            clock,
            counts: Mutex::new(Counts::default()),
            network: watch::Sender::new(network),
            locked: !network,
            slots: Arc::new(Semaphore::new(limits.in_flight)),
            waiting: AtomicUsize::new(0),
        })
    }

    /// The kill switch: nothing more is sent, and what is in flight stops ([`Governor::stopped`]).
    /// Only [`Governor::switch`] turns it on again.
    pub fn stop(&self) {
        self.network.send_replace(false);
    }

    pub fn is_on(&self) -> bool {
        *self.network.borrow()
    }

    /// The app's switch (`:net off`, `:net on`): off is [`Governor::stop`]; on lets requests
    /// through again, unless the network was off from the start or the cap of the run was
    /// reached (then it stays off). Whether the network is now as asked.
    pub fn switch(&self, on: bool) -> bool {
        if !on {
            self.stop();
            return true;
        }
        // Switched on under the counts lock: a trip of the cap cannot land between the check
        // and the switch (and leave the network on after it).
        let counts = self.counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.locked || counts.tripped {
            return false;
        }
        self.network.send_replace(true);
        true
    }

    /// Done when the network is on (at once when it is).
    pub async fn started(&self) {
        let mut rx = self.network.subscribe();
        let _ = rx.wait_for(|on| *on).await;
    }

    /// Done when the network is switched off (at once when it is off).
    pub async fn stopped(&self) {
        let mut rx = self.network.subscribe();
        // The sender lives as long as `self`, so the wait ends only when the switch is off.
        let _ = rx.wait_for(|on| !on).await;
    }

    /// Slack asked to slow down on `method`: it is not sent again for `retry_after`
    /// ([`DEFAULT_PAUSE`] when Slack did not say).
    pub fn pause(&self, method: Method, retry_after: Option<Duration>) {
        let until = self.clock.now() + retry_after.unwrap_or(DEFAULT_PAUSE);
        let mut c = self.counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let at = c.paused.entry(method).or_default();
        *at = (*at).max(until);
    }

    /// How long `method` is still paused, if it is.
    fn paused(&self, method: Method) -> Option<Duration> {
        let now = self.clock.now();
        let c = self.counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        c.paused.get(&method).filter(|until| **until > now).map(|until| *until - now)
    }

    /// A lane of its own for one workspace's requests.
    pub fn lane(&self) -> Lane {
        Lane {
            slots: Arc::new(Semaphore::new(self.limits.lane_in_flight.clamp(1, self.limits.in_flight.max(1)))),
            waiting: AtomicUsize::new(0),
            limit: self.limits.lane_waiting,
            kept: Arc::new(Mutex::new(Arc::clone(&self.slots).try_acquire_owned().ok())),
            background: Mutex::default(),
        }
    }

    /// [`Governor::admit`] for a request of `lane`: it first waits for one of its lane's slots
    /// (refused as busy when its lane's waiting places are taken), then takes the lane's kept
    /// slot when it is free, else waits for one of the process's.
    pub async fn admit_in(&self, lane: &Lane, method: Method, priority: Priority) -> Result<Admitted, SlackError> {
        let off = || SlackError::Offline { why: "the network is switched off" };
        if !self.is_on() {
            return Err(off());
        }
        let Some(place) = Waiting::take(&lane.waiting, lane.limit) else { return Err(SlackError::Busy) };
        let own = tokio::select! {
            slot = Arc::clone(&lane.slots).acquire_owned() => slot.ok(),
            () = self.stopped() => None,
        };
        drop(place);
        let Some(own) = own else { return Err(off()) };
        let slot = lane.kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        let mut admitted = match slot {
            Some(slot) => {
                let kept = Kept { slot: Some(slot), home: Arc::clone(&lane.kept) };
                if let Some(left) = self.paused(method) {
                    return Err(SlackError::RateLimited { retry_after: Some(left) });
                }
                if !self.is_on() {
                    return Err(off());
                }
                self.count(priority, Some(lane))?;
                Admitted { _slot: None, _kept: Some(kept), _lane: None }
            }
            None => self.admit_for(method, priority, Some(lane)).await?,
        };
        admitted._lane = Some(own);
        Ok(admitted)
    }

    /// Let a request for `method` of `priority` through, waiting for a slot when all are
    /// taken: nothing may be sent without it.
    pub async fn admit(&self, method: Method, priority: Priority) -> Result<Admitted, SlackError> {
        self.admit_for(method, priority, None).await
    }

    /// [`Governor::admit`], counted on `lane`'s share as well when it came through one.
    async fn admit_for(&self, method: Method, priority: Priority, lane: Option<&Lane>) -> Result<Admitted, SlackError> {
        let off = || SlackError::Offline { why: "the network is switched off" };
        if !self.is_on() {
            return Err(off());
        }
        if let Some(left) = self.paused(method) {
            return Err(SlackError::RateLimited { retry_after: Some(left) });
        }
        let Some(place) = Waiting::take(&self.waiting, self.limits.waiting) else { return Err(SlackError::Busy) };
        let slot = tokio::select! {
            slot = Arc::clone(&self.slots).acquire_owned() => slot,
            () = self.stopped() => return Err(off()),
        };
        drop(place);
        let Ok(slot) = slot else { return Err(off()) };
        if !self.is_on() {
            return Err(off());
        }
        self.count(priority, lane)?;
        Ok(Admitted { _slot: Some(slot), _kept: None, _lane: None })
    }

    /// Count a request (of `lane`, when it came through one) against the caps, or refuse it.
    fn count(&self, priority: Priority, lane: Option<&Lane>) -> Result<(), SlackError> {
        let now = self.clock.now();
        let mut c = self.counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if c.tripped {
            return Err(SlackError::CapReached { why: "the request cap of this run is reached" });
        }
        while c.window.front().is_some_and(|at| now.saturating_sub(*at) >= HOUR) {
            c.window.pop_front();
        }
        let hour = u32::try_from(c.window.len()).unwrap_or(u32::MAX);
        if hour >= self.limits.per_hour {
            return Err(SlackError::CapReached { why: "the hourly request budget is reached" });
        }
        if priority == Priority::Background && hour >= self.limits.background_per_hour {
            return Err(SlackError::CapReached { why: "the hourly background share is reached" });
        }
        // Background work stops once the minute holds its share, whoever sent those: the rest
        // of the minute stays free for the user.
        let used = c.window.iter().rev().take_while(|at| now.saturating_sub(**at) < WINDOW).count();
        let used = u32::try_from(used).unwrap_or(u32::MAX);
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
        // A workspace's background work keeps within its part of the shares.
        if let Some(lane) = lane.filter(|_| priority == Priority::Background) {
            let mut own = lane.background.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            while own.front().is_some_and(|at| now.saturating_sub(*at) >= HOUR) {
                own.pop_front();
            }
            let minute = own.iter().rev().take_while(|at| now.saturating_sub(**at) < WINDOW).count();
            if u32::try_from(minute).unwrap_or(u32::MAX) >= self.limits.lane_background_per_minute
                || u32::try_from(own.len()).unwrap_or(u32::MAX) >= self.limits.lane_background_per_hour
            {
                return Err(SlackError::CapReached { why: "this workspace's background share is reached" });
            }
            own.push_back(now);
        }
        c.total += 1;
        c.window.push_back(now);
        Ok(())
    }

    /// The live updates' socket may open now: the network is on and the caps have room. An
    /// opening counts as one background request; what comes over the socket afterwards is not a
    /// request and is not counted.
    /// It counts on `lane`'s background share too.
    pub fn dial(&self, lane: &Lane) -> Result<(), SlackError> {
        if !self.is_on() {
            return Err(SlackError::Offline { why: "the network is switched off" });
        }
        self.count(Priority::Background, Some(lane))
    }

    /// Requests let through so far.
    pub fn sent(&self) -> u32 {
        self.counts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).total
    }
}

#[cfg(test)]
mod tests;
