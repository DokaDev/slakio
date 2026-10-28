//! A workspace's live updates: one socket to Slack's realtime host, opened once the workspace has
//! booted, which only receives — new, changed and deleted messages, conversations read elsewhere
//! — and pushes them to the UI as [`Event::Live`]. The client sends nothing over it but what its
//! protocol needs (a pong for each ping, a close); no message, typing or read mark ever goes out.
//!
//! The socket replays nothing it missed, so when it opens again after it was lost the UI is told
//! to read its open conversations again (`Connected { catch_up: true }`). It opens again by
//! itself after a pause that doubles from [`Realtime::first_retry`] up to
//! [`Realtime::longest_retry`], each with a random part (so many clients do not come back at
//! once); the pause starts over only after a socket stayed open [`Realtime::steady`]. A socket
//! silent for [`Realtime::silence`] (Slack pings it twice a minute) is taken as lost. Every
//! opening passes the governor: with the network off nothing opens, and switching it off ends the
//! socket; an opening counts against the request cap. A workspace's socket is its own: one that
//! fails holds no other back.
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
    /// A socket open this long resets the pause to the first one.
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

/// The longest a new author's name is waited for before their message is shown with their id.
const NAME_WAIT: Duration = Duration::from_secs(5);

/// How a socket ended.
enum End {
    /// The network was switched off, or the UI is gone: no more sockets.
    Stop,
    /// It was lost (or never opened); `opened`: it said hello first.
    Lost { opened: bool },
}

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
    let mut jitter = RandomIds::default();
    let (mut failures, mut ever, mut told_lost) = (0u32, false, false);
    loop {
        let began = Instant::now();
        match connection(&client, &ctx, &ws, &tx, settings, ever).await {
            End::Stop => return,
            // Said once each time: lost after it was open, or not open (yet, or again).
            End::Lost { opened } => {
                if opened {
                    (ever, told_lost) = (true, false);
                }
                if !told_lost {
                    told_lost = true;
                    if push(&tx, Live::Connection { workspace: ws.clone(), state: Connection::Lost }).await.is_err() {
                        return;
                    }
                }
            }
        }
        failures = if began.elapsed() >= settings.steady { 0 } else { failures.saturating_add(1) };
        let r = f64::from(u16::from_str_radix(&jitter.hex(2), 16).unwrap_or(0)) / 65536.0;
        tokio::select! {
            () = tokio::time::sleep(settings.retry_in(failures.saturating_sub(1), r)) => {}
            () = client.governor().stopped() => return,
            () = tx.closed() => return,
        }
    }
}

/// Push `live` to the UI; `Err` when it is gone.
async fn push(tx: &mpsc::Sender<Envelope>, live: Live) -> Result<(), ()> {
    let envelope = Envelope { generation: Generation::PUSHED, event: Event::Live(Box::new(live)) };
    tx.send(envelope).await.map_err(|_| ())
}

/// One socket, from its opening to its end. `ever`: a socket of this workspace opened before
/// (what it missed since should be read again).
async fn connection(
    client: &Client,
    ctx: &Session,
    ws: &WorkspaceId,
    tx: &mpsc::Sender<Envelope>,
    settings: Realtime,
    ever: bool,
) -> End {
    let stopped = client.governor().stopped();
    let opened = tokio::select! {
        socket = client.open_socket() => socket,
        () = stopped => return End::Stop,
    };
    let mut socket = match opened {
        Ok(io) => Socket::new(io),
        Err(crate::SlackError::Offline { .. }) if !client.governor().is_on() => return End::Stop,
        Err(_) => return End::Lost { opened: false },
    };
    let mut hello = false;
    // Each new author is looked up once a socket.
    let mut asked: HashSet<String> = HashSet::new();
    loop {
        let next = tokio::select! {
            next = tokio::time::timeout(settings.silence, socket.next()) => next,
            () = client.governor().stopped() => return End::Stop,
            () = tx.closed() => return End::Stop,
        };
        let text = match next {
            Ok(Ok(Incoming::Text(text))) => text,
            Ok(Ok(Incoming::Alive)) => continue,
            Ok(Ok(Incoming::Closed) | Err(_)) => return End::Lost { opened: hello },
            // Silent too long: gone, as far as anyone can tell.
            Err(_) => {
                socket.close().await;
                return End::Lost { opened: hello };
            }
        };
        let me = ctx.me.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let live = match events::frame(&text, me.as_ref()) {
            Frame::Hello if !hello => {
                hello = true;
                Live::Connection { workspace: ws.clone(), state: Connection::Connected { catch_up: ever } }
            }
            Frame::Goodbye => {
                socket.close().await;
                return End::Lost { opened: hello };
            }
            Frame::Message { conversation, thread, broadcast, changed, mention, message, bots } => {
                let author = message.user.as_str().to_string();
                let mut users: Vec<User> = bots.into_iter().map(|(id, b)| bot(ws.as_str(), id, b)).collect();
                let unknown =
                    users.is_empty() && !author.is_empty() && me.as_ref().is_none_or(|m| m.as_str() != author);
                if unknown && asked.insert(author.clone()) {
                    let ids = [author];
                    let lookup = ctx.people.names(client, &ids, Priority::Background);
                    users.extend(tokio::time::timeout(NAME_WAIT, lookup).await.unwrap_or_default());
                }
                let conversation = ConversationId::new(conversation);
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
        if push(tx, live).await.is_err() {
            return End::Stop;
        }
    }
}

#[cfg(test)]
mod tests;
