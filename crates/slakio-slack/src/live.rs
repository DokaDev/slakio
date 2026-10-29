//! A workspace's live updates: one socket to Slack's realtime host, opened once the workspace has
//! booted, which only receives — new, changed and deleted messages, conversations read elsewhere
//! — and pushes them to the UI as [`Event::Live`]. The client sends nothing over it but what its
//! protocol needs (a pong for each ping, a close); no message, typing or read mark ever goes out.
//!
//! The socket replays nothing it missed, so when it opens after it was said to be lost (or not
//! connected yet) the UI is told to read its open conversations again (`Connected { catch_up:
//! true }`). It opens again by itself after a pause that doubles from [`Realtime::first_retry`]
//! up to [`Realtime::longest_retry`], each with a random part (so many clients do not come back at
//! once), and never shorter than Slack asked for; the pause starts over only after a socket said
//! hello and stayed open [`Realtime::steady`]. A socket
//! silent for [`Realtime::silence`] (Slack pings it twice a minute) is taken as lost; so is one
//! open while the machine slept (a laptop's lid closed: the wall clock ran ahead of the monotonic
//! one, [`Realtime::slept`]), at once, and it opens again without a pause. After a socket was
//! lost, Slack's unread counts are read again too (one `client.counts`), since what came
//! meanwhile in conversations not open is counted nowhere else. Every
//! opening passes the governor: with the network off nothing opens, and switching it off ends the
//! socket (switched on again, it opens again and asks to catch up); an opening counts against the request cap, and on the workspace's own part of the
//! background share (while that has no room, the UI is told so: `Held`, not just lost). A workspace's socket is its own: one that fails holds no other back.
//! Once Slack no longer accepts the workspace's sign-in (a request or the socket's opening said
//! so), the socket closes, the UI is told `SignedOut` once, and nothing opens again.
//!
//! Derived from gammons/slk (MIT, Copyright (c) 2026 Grant Ammons): the pause doubling from 1 s
//! to 30 s, the 60 s silence limit, and that the socket replays nothing.

pub(crate) mod events;
pub(crate) mod socket;

use crate::adapter::{Session, bot};
use crate::client::Client;
use crate::governor::Priority;
use crate::request::{Ids, RandomIds};
use events::Frame;
use slakio_core::backend::{Connection, Envelope, Event, Generation, Live};
use slakio_core::model::{ConversationId, User, WorkspaceId};
use socket::{Incoming, Socket};
use std::collections::HashSet;
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::mpsc;

/// How the live updates connect, and connect again.
#[derive(Clone, Copy, Debug)]
pub struct Realtime {
    /// The pause before the first new attempt.
    pub first_retry: Duration,
    /// The longest pause between attempts.
    pub longest_retry: Duration,
    /// A socket open this long after its hello resets the pause to the first one.
    pub steady: Duration,
    /// A socket that says nothing this long is lost.
    pub silence: Duration,
    /// How often the clocks are compared while a socket is open.
    pub wake_check: Duration,
    /// The wall clock ahead of the monotonic one by more than this between two checks: the
    /// machine slept.
    pub wake_gap: Duration,
    /// The wall clock (the tests' moves when they say).
    pub wall: fn() -> SystemTime,
}

impl Default for Realtime {
    fn default() -> Self {
        Self {
            first_retry: Duration::from_secs(1),
            longest_retry: Duration::from_secs(30),
            steady: Duration::from_secs(30),
            silence: Duration::from_secs(60),
            wake_check: Duration::from_secs(5),
            wake_gap: Duration::from_secs(10),
            wall: SystemTime::now,
        }
    }
}

impl Realtime {
    /// The pause after `failures` attempts in a row that failed (or did not stay open), with
    /// `jitter` in `0.0..1.0`: between half the doubled pause and all of it, never longer than
    /// [`Realtime::longest_retry`].
    pub fn retry_in(&self, failures: u32, jitter: f64) -> Duration {
        let doubled = self.first_retry.saturating_mul(1 << failures.min(16)).min(self.longest_retry);
        doubled / 2 + doubled.mul_f64(jitter.clamp(0.0, 1.0)) / 2
    }

    /// The machine slept between two checks `ran` apart on the monotonic clock (which stops
    /// while it sleeps), the wall clock reading `before` and `after` (which does not): the wall
    /// clock moved on more than [`Realtime::wake_gap`] past the monotonic one. A wall clock set
    /// back is no sleep.
    pub fn slept(&self, ran: Duration, before: SystemTime, after: SystemTime) -> bool {
        after.duration_since(before).is_ok_and(|wall| wall > ran.saturating_add(self.wake_gap))
    }
}

/// Done when the machine slept while waiting ([`Realtime::slept`], checked every
/// [`Realtime::wake_check`]).
async fn woke(settings: Realtime) {
    let mut at = (tokio::time::Instant::now(), (settings.wall)());
    loop {
        tokio::time::sleep(settings.wake_check).await;
        let now = (tokio::time::Instant::now(), (settings.wall)());
        if settings.slept(now.0 - at.0, at.1, now.1) {
            return;
        }
        at = now;
    }
}

/// The longest new authors' names are waited for before their messages are shown with their ids.
const NAME_WAIT: Duration = Duration::from_secs(5);

/// The longest a close of the client's own is waited for on a socket that may be gone.
const CLOSE_WAIT: Duration = Duration::from_secs(1);

/// How many updates wait to go to the UI (while names are looked up) before the socket's reading
/// waits too.
const OUTBOX: usize = 256;

/// How a socket ended.
enum End {
    /// The UI is gone: no more sockets.
    Stop,
    /// The network was switched off: none opens until it is on again.
    Off,
    /// It was lost (or never opened). `hello`: when it said hello, if it did; `wait`: how long
    /// Slack asked to wait before the next attempt; `held`: it was not opened because the
    /// workspace's share of the request budget is used up; `woke`: the machine slept.
    Lost { hello: Option<Instant>, wait: Option<Duration>, held: bool, woke: bool },
    /// Slack no longer accepts the sign-in: none opens again.
    SignedOut,
}

/// An update on its way to the UI, and the author whose name it waits for, if one.
type Out = (Live, Option<String>);

/// Run the live updates of `client`'s workspace until the network is switched off or the UI is
/// gone, pushing them on `tx`.
pub(crate) async fn run(client: Arc<Client>, ctx: Arc<Session>, tx: mpsc::Sender<Envelope>, settings: Realtime) {
    // The socket names the team and the user: it opens once the workspace has booted.
    let ws = loop {
        if let Some(team) = ctx.people.team()
            && ctx.me.lock().unwrap_or_else(PoisonError::into_inner).is_some()
        {
            break WorkspaceId::new(team);
        }
        // The network may be switched off and on before the boot: it waits for it all the same.
        tokio::select! {
            () = ctx.booted.notified() => {}
            () = tx.closed() => return,
            () = client.signed_out() => return,
        }
    };
    // Updates go out in order through a task of their own, which looks up new authors: reading
    // the socket (and answering its pings) never waits for a lookup.
    let (out, rx) = mpsc::channel::<Out>(OUTBOX);
    tokio::spawn(outbox(Arc::clone(&client), Arc::clone(&ctx), rx, tx));
    let mut jitter = RandomIds::default();
    // What the UI was last told while not connected: lost, or held by the budget; and whether
    // the network was switched off since the last hello.
    let (mut failures, mut told, mut off) = (0u32, None, false);
    // When the machine last woke: a wake starts the pause over only once in a while.
    let mut last_wake: Option<Instant> = None;
    loop {
        // What was missed while it was said to be lost (or not connected yet), or while the
        // network was off, is read again.
        let ended = connection(&client, &ctx, &ws, &out, settings, told.is_some() || off).await;
        let (hello, wait, held, woke) = match ended {
            End::Stop => return,
            End::SignedOut => {
                let _ = out.send((Live::Connection { workspace: ws, state: Connection::SignedOut }, None)).await;
                return;
            }
            End::Off => {
                off = true;
                tokio::select! {
                    () = client.governor().started() => continue,
                    () = out.closed() => return,
                }
            }
            End::Lost { hello, wait, held, woke } => (hello, wait, held, woke),
        };
        if hello.is_some() {
            (told, off) = (None, false);
        }
        // Said once each time it changes: lost after it was open, or not open (yet, or again),
        // or held until the budget has room.
        let state = if held { Connection::Held } else { Connection::Lost };
        if told != Some(state) {
            told = Some(state);
            let lost = Live::Connection { workspace: ws.clone(), state };
            if out.send((lost, None)).await.is_err() {
                return;
            }
        }
        // The pause starts over only after a socket said hello and stayed open a while, or when
        // the machine woke (the network it slept on is not the one it may wake on) at least that
        // long after it last woke: a clock that keeps jumping on does not keep the pause short.
        let fresh_wake = woke && last_wake.is_none_or(|at| at.elapsed() >= settings.steady);
        if woke {
            last_wake = Some(Instant::now());
        }
        failures = if fresh_wake || hello.is_some_and(|at| at.elapsed() >= settings.steady) {
            0
        } else {
            failures.saturating_add(1)
        };
        let r = f64::from(u16::from_str_radix(&jitter.hex(2), 16).unwrap_or(0)) / 65536.0;
        let pause = settings.retry_in(failures.saturating_sub(1), r).max(wait.unwrap_or_default());
        tokio::select! {
            () = tokio::time::sleep(pause) => {}
            () = client.governor().stopped() => off = true,
            () = out.closed() => return,
            // Told at the next opening, which sends nothing.
            () = client.signed_out() => continue,
        }
        if off {
            tokio::select! {
                () = client.governor().started() => {}
                () = out.closed() => return,
            }
        }
    }
}

/// Slack's unread counts of `ws` read again after its socket came back (one `client.counts`, in
/// the background share), pushed on `out`; when they cannot be read, the counts stay as they are.
async fn recount(client: Arc<Client>, ws: WorkspaceId, out: mpsc::Sender<Out>) {
    let Some(counts) = crate::boot::counts(&client).await else { return };
    let mut counts: Vec<(ConversationId, u32, u32)> = crate::boot::unread_counts(counts)
        .into_iter()
        .map(|(id, (unread, mentions))| (ConversationId::new(id), unread, mentions))
        .collect();
    counts.sort_unstable_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    let _ = out.send((Live::Counts { workspace: ws, counts }, None)).await;
}

/// Push `live` to the UI; `Err` when it is gone.
async fn push(tx: &mpsc::Sender<Envelope>, live: Live) -> Result<(), ()> {
    let envelope = Envelope { generation: Generation::PUSHED, event: Event::Live(Box::new(live)) };
    tx.send(envelope).await.map_err(|_| ())
}

/// Hand the updates on `rx` to the UI on `tx`, in order. The new authors of those waiting are
/// looked up together (in the background share of the budget, [`NAME_WAIT`] at most), each once
/// a run, so their messages show their names.
async fn outbox(client: Arc<Client>, ctx: Arc<Session>, mut rx: mpsc::Receiver<Out>, tx: mpsc::Sender<Envelope>) {
    let mut asked: HashSet<String> = HashSet::new();
    while let Some(first) = rx.recv().await {
        let mut held = vec![first];
        while held.len() < OUTBOX
            && let Ok(more) = rx.try_recv()
        {
            held.push(more);
        }
        let wanted: Vec<String> =
            held.iter().filter_map(|(_, who)| who.clone()).filter(|who| asked.insert(who.clone())).collect();
        if !wanted.is_empty() {
            let lookup = ctx.people.names(&client, &wanted, Priority::Background);
            let named = tokio::time::timeout(NAME_WAIT, lookup).await.unwrap_or_default();
            for (live, who) in &mut held {
                if let (Live::Message { users, .. }, Some(who)) = (live, who.as_ref()) {
                    users.extend(named.iter().filter(|u| u.id.as_str() == who).cloned());
                }
            }
        }
        for (live, _) in held {
            if push(&tx, live).await.is_err() {
                return;
            }
        }
    }
}

/// Open `client`'s socket, or how that ended.
async fn open(client: &Client) -> Result<Socket<crate::http::Upgraded>, End> {
    let stopped = client.governor().stopped();
    let opened = tokio::select! {
        biased;
        () = stopped => return Err(End::Off),
        socket = client.open_socket() => socket,
    };
    match opened {
        Ok(io) => Ok(Socket::new(io)),
        Err(crate::SlackError::Offline { .. }) if !client.governor().is_on() => Err(End::Off),
        Err(crate::SlackError::Auth { .. }) => Err(End::SignedOut),
        // Slack asked to slow down: not before it said (its default pause when it did not).
        Err(crate::SlackError::RateLimited { retry_after }) => {
            let wait = retry_after.unwrap_or(crate::governor::DEFAULT_PAUSE);
            Err(End::Lost { hello: None, wait: Some(wait), held: false, woke: false })
        }
        // The budget has no room for it: tried again after the pause, until it has.
        Err(crate::SlackError::CapReached { .. }) => {
            Err(End::Lost { hello: None, wait: None, held: true, woke: false })
        }
        Err(_) => Err(End::Lost { hello: None, wait: None, held: false, woke: false }),
    }
}

/// Close `socket` after the machine woke ([`CLOSE_WAIT`] at most), and how it ended: lost, or
/// off when the network is switched off (then the close is not written).
async fn close_after_wake(client: &Client, socket: &mut Socket<crate::http::Upgraded>, hello: Option<Instant>) -> End {
    tokio::select! {
        biased;
        () = client.governor().stopped() => End::Off,
        _ = tokio::time::timeout(CLOSE_WAIT, socket.close()) => End::Lost { hello, wait: None, held: false, woke: true },
    }
}

/// One socket, from its opening to its end, its updates sent to `out`. `catch_up`: what came
/// before it opened should be read again.
async fn connection(
    client: &Arc<Client>,
    ctx: &Session,
    ws: &WorkspaceId,
    out: &mpsc::Sender<Out>,
    settings: Realtime,
    catch_up: bool,
) -> End {
    let mut socket = match open(client).await {
        Ok(socket) => socket,
        Err(end) => return end,
    };
    let mut hello = None;
    let lost = |hello| End::Lost { hello, wait: None, held: false, woke: false };
    let wake = woke(settings);
    tokio::pin!(wake);
    loop {
        // The switch is looked at first: once it is off, not even an answer to a ping goes out.
        let next = tokio::select! {
            biased;
            () = client.governor().stopped() => return End::Off,
            () = out.closed() => return End::Stop,
            // A request learnt that Slack no longer takes the sign-in: the socket closes too.
            () = client.signed_out() => {
                let _ = tokio::time::timeout(CLOSE_WAIT, socket.close()).await;
                return End::SignedOut;
            }
            next = tokio::time::timeout(settings.silence, socket.next()) => next,
            // The machine slept: whatever the socket was, it is gone by now.
            () = &mut wake => return close_after_wake(client, &mut socket, hello).await,
        };
        let text = match next {
            Ok(Ok(Incoming::Text(text))) => text,
            Ok(Ok(Incoming::Alive)) => continue,
            Ok(Ok(Incoming::Closed) | Err(_)) => return lost(hello),
            // Silent too long: gone, as far as anyone can tell.
            Err(_) => {
                socket.close().await;
                return lost(hello);
            }
        };
        let me = ctx.me.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let mut who = None;
        let live = match events::frame(&text, me.as_ref()) {
            Frame::Hello if hello.is_none() => {
                hello = Some(Instant::now());
                if catch_up {
                    tokio::spawn(recount(Arc::clone(client), ws.clone(), out.clone()));
                }
                Live::Connection { workspace: ws.clone(), state: Connection::Connected { catch_up } }
            }
            Frame::Goodbye => {
                socket.close().await;
                return lost(hello);
            }
            Frame::Message { conversation, thread, broadcast, changed, mention, message, bots } => {
                let author = message.user.as_str().to_string();
                let users: Vec<User> = bots.into_iter().map(|(id, b)| bot(ws.as_str(), id, b)).collect();
                let conversation = ConversationId::new(conversation);
                // A new author is named only in a conversation the user read: the socket brings
                // every conversation's messages, and a large workspace has many authors.
                let opened = ctx.opened.lock().unwrap_or_else(PoisonError::into_inner).contains(&conversation);
                if opened && users.is_empty() && !author.is_empty() && me.as_ref().is_none_or(|m| m.as_str() != author)
                {
                    who = Some(author);
                }
                Live::Message {
                    workspace: ws.clone(),
                    conversation,
                    thread,
                    broadcast,
                    changed,
                    mention,
                    message,
                    users,
                }
            }
            Frame::Deleted { conversation, thread, ts } => {
                Live::Deleted { workspace: ws.clone(), conversation: ConversationId::new(conversation), thread, ts }
            }
            Frame::Marked { conversation, unread, mentions } => Live::Marked {
                workspace: ws.clone(),
                conversation: ConversationId::new(conversation),
                unread,
                mentions,
            },
            Frame::Hello | Frame::Other => continue,
        };
        if out.send((live, who)).await.is_err() {
            return End::Stop;
        }
    }
}

#[cfg(test)]
mod tests;
