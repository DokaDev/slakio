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
//! silent for [`Realtime::silence`] (Slack pings it twice a minute) is taken as lost. Every
//! opening passes the governor: with the network off nothing opens, and switching it off ends the
//! socket; an opening counts against the request cap, and on the workspace's own part of the
//! background share (while that has no room, the UI is told so: `Held`, not just lost). A workspace's socket is its own: one that fails holds no other back.
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
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// How the live updates connect, and connect again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Realtime {
    /// The pause before the first new attempt.
    pub first_retry: Duration,
    /// The longest pause between attempts.
    pub longest_retry: Duration,
    /// A socket open this long after its hello resets the pause to the first one.
    pub steady: Duration,
    /// A socket that says nothing this long is lost.
    pub silence: Duration,
}

impl Default for Realtime {
    fn default() -> Self {
        Self {
            first_retry: Duration::from_secs(1),
            longest_retry: Duration::from_secs(30),
            steady: Duration::from_secs(30),
            silence: Duration::from_secs(60),
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
}

/// The longest new authors' names are waited for before their messages are shown with their ids.
const NAME_WAIT: Duration = Duration::from_secs(5);

/// How many updates wait to go to the UI (while names are looked up) before the socket's reading
/// waits too.
const OUTBOX: usize = 256;

/// How a socket ended.
enum End {
    /// The network was switched off, or the UI is gone: no more sockets.
    Stop,
    /// It was lost (or never opened). `hello`: when it said hello, if it did; `wait`: how long
    /// Slack asked to wait before the next attempt; `held`: it was not opened because the
    /// workspace's share of the request budget is used up.
    Lost { hello: Option<Instant>, wait: Option<Duration>, held: bool },
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
        tokio::select! {
            () = ctx.booted.notified() => {}
            () = client.governor().stopped() => return,
            () = tx.closed() => return,
        }
    };
    // Updates go out in order through a task of their own, which looks up new authors: reading
    // the socket (and answering its pings) never waits for a lookup.
    let (out, rx) = mpsc::channel::<Out>(OUTBOX);
    tokio::spawn(outbox(Arc::clone(&client), Arc::clone(&ctx), rx, tx));
    let mut jitter = RandomIds::default();
    // What the UI was last told while not connected: lost, or held by the budget.
    let (mut failures, mut told) = (0u32, None);
    loop {
        // What was missed while it was said to be lost (or not connected yet) is read again.
        let ended = connection(&client, &ctx, &ws, &out, settings, told.is_some()).await;
        let End::Lost { hello, wait, held } = ended else { return };
        if hello.is_some() {
            told = None;
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
        // The pause starts over only after a socket said hello and stayed open a while.
        failures = if hello.is_some_and(|at| at.elapsed() >= settings.steady) { 0 } else { failures.saturating_add(1) };
        let r = f64::from(u16::from_str_radix(&jitter.hex(2), 16).unwrap_or(0)) / 65536.0;
        let pause = settings.retry_in(failures.saturating_sub(1), r).max(wait.unwrap_or_default());
        tokio::select! {
            () = tokio::time::sleep(pause) => {}
            () = client.governor().stopped() => return,
            () = out.closed() => return,
        }
    }
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

/// One socket, from its opening to its end, its updates sent to `out`. `catch_up`: what came
/// before it opened should be read again.
async fn connection(
    client: &Client,
    ctx: &Session,
    ws: &WorkspaceId,
    out: &mpsc::Sender<Out>,
    settings: Realtime,
    catch_up: bool,
) -> End {
    let stopped = client.governor().stopped();
    let opened = tokio::select! {
        socket = client.open_socket() => socket,
        () = stopped => return End::Stop,
    };
    let mut socket = match opened {
        Ok(io) => Socket::new(io),
        Err(crate::SlackError::Offline { .. }) if !client.governor().is_on() => return End::Stop,
        // Slack asked to slow down: not before it said (its default pause when it did not).
        Err(crate::SlackError::RateLimited { retry_after }) => {
            let wait = retry_after.unwrap_or(crate::governor::DEFAULT_PAUSE);
            return End::Lost { hello: None, wait: Some(wait), held: false };
        }
        // The budget has no room for it: tried again after the pause, until it has.
        Err(crate::SlackError::CapReached { .. }) => return End::Lost { hello: None, wait: None, held: true },
        Err(_) => return End::Lost { hello: None, wait: None, held: false },
    };
    let mut hello = None;
    let lost = |hello| End::Lost { hello, wait: None, held: false };
    loop {
        let next = tokio::select! {
            next = tokio::time::timeout(settings.silence, socket.next()) => next,
            () = client.governor().stopped() => return End::Stop,
            () = out.closed() => return End::Stop,
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
