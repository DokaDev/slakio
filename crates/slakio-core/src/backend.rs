//! The protocol between the UI and a backend (the demo world now, Slack later): the UI sends
//! [`Request`]s and receives [`Event`]s, each tagged with the id ([`Generation`]) of the request
//! it answers so an answer that arrives after a newer request is dropped instead of drawn.
//!
//! The UI state never calls a backend: it queues requests, and the binary's loop delivers them
//! through a [`BackendHandle`] and hands the events back whenever they arrive (an answer may come
//! late, or never). Only one wiring file of the UI names a concrete backend.

use crate::model::{
    Conversation, ConversationId, Message, SavedId, SavedItem, Section, Target, Ts, User, Workspace, WorkspaceId,
};
use std::time::Duration;
use tokio::sync::mpsc;

/// Which request an event answers. The UI bumps it for every new request of the same kind and
/// drops events of an older one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Generation(pub u64);

impl Generation {
    /// What an event no request asked for carries ([`Event::Live`]): no request has it.
    pub const PUSHED: Generation = Generation(0);

    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// What a backend can do, so the UI offers only that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// The data is invented (the demo): the status line says so, and nothing reaches Slack.
    pub demo: bool,
    /// Nothing can be changed: a [`Command::Save`] is answered with [`FailKind::ReadOnly`], and
    /// the UI offers no change.
    pub read_only: bool,
}

/// A command, with the id its answers carry and the workspace it is about (`None`: every one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: Generation,
    pub workspace: Option<WorkspaceId>,
    pub command: Command,
}

/// A request to the backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Load the workspaces, people, sidebar sections and conversations: of the request's
    /// workspace when it names one (answered with that workspace's part only), else of every
    /// workspace the backend has.
    Boot,
    /// Up to `limit` messages of `target` older than `before` (the newest when `None`). Answered
    /// with [`Event::History`].
    History { target: Target, before: Option<Ts>, limit: u32 },
    /// Change saved item `item` (Slack's `saved.update` / `saved.delete`). Answered with
    /// [`Event::Saved`], every saved item after the change.
    Save { item: SavedId, change: SavedChange },
    /// The newest `limit` messages of the conversation `target` again, after live updates were
    /// lost a while (they replay nothing): answered with [`Event::History`], read in the
    /// background. Unlike an open, it never waits for or supersedes one.
    Catchup { target: Target, limit: u32 },
}

impl Command {
    /// The command asks the backend to change something (a read-only backend refuses it). Every
    /// command is named here, so a new one is classified when it is added.
    pub fn changes(&self) -> bool {
        match self {
            Command::Boot | Command::History { .. } | Command::Catchup { .. } => false,
            Command::Save { .. } => true,
        }
    }
}

/// A change to a saved item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavedChange {
    /// A to-do in progress is completed.
    Complete,
    /// A completed to-do is in progress again.
    Reopen,
    /// Out of To-dos into Saved; its reminder goes.
    MoveToSaved,
    /// Out of Saved into To-dos, in progress.
    MoveToTodos,
    /// Gone from To-dos and Saved.
    Clear,
}

/// A page of messages: the answer to [`Command::History`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub target: Target,
    /// Oldest first. A thread's first page (the oldest) starts with the thread's own message.
    pub messages: Vec<Message>,
    /// Nothing older exists: the start of the conversation or thread is in this page.
    pub complete: bool,
    /// People the messages name whom the backend learned of with this page (their names, for
    /// authors not in the snapshot); none when it has nothing new.
    pub users: Vec<User>,
    /// A thread read only in part (longer than is read at once, or Slack gave no way on): its
    /// newest replies are not among those read, and the page is no word on what is newer.
    pub cut: bool,
}

/// Everything the UI lists right after start: the answer to [`Command::Boot`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// In the view switcher's order.
    pub workspaces: Vec<Workspace>,
    pub users: Vec<User>,
    /// Per workspace, in sidebar order.
    pub sections: Vec<Section>,
    /// Per section, in sidebar order.
    pub conversations: Vec<Conversation>,
    /// The to-dos and saved messages of every workspace; `None` when the backend cannot tell
    /// (unknown is not empty).
    pub saved: Option<Vec<SavedItem>>,
    /// When the backend took this snapshot: reminders due before it are overdue.
    pub taken: Ts,
    /// The unread counts could not be read: every conversation's `unread` and `mentions` here
    /// are unknown, not zero (the UI keeps those it had, and says so).
    pub counts_failed: bool,
}

impl Snapshot {
    /// What of this snapshot belongs to workspace `ws` (its own boot answer, when every
    /// workspace boots on its own); the clock is kept.
    pub fn only(&self, ws: &WorkspaceId) -> Snapshot {
        Snapshot {
            workspaces: self.workspaces.iter().filter(|w| &w.id == ws).cloned().collect(),
            users: self.users.iter().filter(|u| &u.workspace == ws).cloned().collect(),
            sections: self.sections.iter().filter(|s| &s.workspace == ws).cloned().collect(),
            conversations: self.conversations.iter().filter(|c| &c.workspace == ws).cloned().collect(),
            saved: self.saved.as_ref().map(|items| items.iter().filter(|s| &s.workspace == ws).cloned().collect()),
            taken: self.taken,
            counts_failed: self.counts_failed,
        }
    }
}

/// Something the backend tells the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Booted(Box<Snapshot>),
    History(Box<Page>),
    /// The answer to a [`Command::Boot`] as the local cache kept it from an earlier run, given
    /// before the request's own answer so a start draws at once: shown until that answer (its
    /// `Booted`) replaces it. Never the request's answer: that one still comes, or its `Failed`.
    CachedBoot(Box<Snapshot>),
    /// The newest page of a conversation as the local cache kept it, given before the request's
    /// own answer (its `History`, or `Failed`), which replaces it. Like [`Event::CachedBoot`], it
    /// ends nothing.
    CachedHistory(Box<Page>),
    /// Every saved item after a [`Command::Save`], and the backend's clock then (reminders due
    /// before it are overdue).
    Saved {
        items: Vec<SavedItem>,
        taken: Ts,
    },
    /// Request `request` failed: nothing changed. `detail` is raw text for the error log only.
    Failed {
        request: Generation,
        kind: FailKind,
        detail: String,
    },
    /// Something that happened in a workspace, as it happened (no request asked for it; its
    /// envelope carries [`Generation::PUSHED`]).
    Live(Box<Live>),
}

/// What a backend's live updates tell of a workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Live {
    /// A message posted in `conversation` — a reply under message `thread`, also shown in the
    /// conversation when `broadcast` — or, when `changed`, a new copy of one (edited, or its
    /// replies changed): it replaces the copy loaded and is never added. `mention`: it mentions
    /// the user. `users`: its author when the UI may not know them.
    Message {
        workspace: WorkspaceId,
        conversation: ConversationId,
        thread: Option<Ts>,
        broadcast: bool,
        changed: bool,
        mention: bool,
        message: Message,
        users: Vec<User>,
    },
    /// The message at `ts` was deleted (a reply under `thread`, when Slack said).
    Deleted { workspace: WorkspaceId, conversation: ConversationId, thread: Option<Ts>, ts: Ts },
    /// The conversation was read elsewhere (another client): its counts now.
    Marked { workspace: WorkspaceId, conversation: ConversationId, unread: u32, mentions: u32 },
    /// The live updates of `workspace` started, or stopped.
    Connection { workspace: WorkspaceId, state: Connection },
    /// Slack's unread counts of `workspace`, read again after its live updates came back (what
    /// came meanwhile in conversations not open is counted there): (conversation, unread,
    /// mentions) of each conversation Slack listed; one it did not list has none, as at a boot.
    Counts { workspace: WorkspaceId, counts: Vec<(ConversationId, u32, u32)> },
}

/// Whether a workspace's live updates arrive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    /// They arrive; `catch_up`: they were lost a while, and what was missed meanwhile should be
    /// read again ([`Command::Catchup`]).
    Connected { catch_up: bool },
    /// They stopped; the backend connects again by itself.
    Lost,
    /// They stopped and cannot connect for now: the workspace's share of the request budget is
    /// used up. The backend tries again by itself once there is room.
    Held,
    /// Slack no longer accepts the workspace's sign-in: nothing more is sent for it, and the
    /// backend never connects again (a new sign-in starts a new backend).
    SignedOut,
}

impl FailKind {
    /// Asking again may work (when the user asks: nothing retries on its own).
    pub fn retryable(&self) -> bool {
        match self {
            FailKind::Network | FailKind::Offline | FailKind::RateLimited { .. } | FailKind::Busy => true,
            FailKind::ReadOnly | FailKind::Auth | FailKind::CapReached | FailKind::Api { .. } => false,
        }
    }
}

/// Why a request failed, as far as the UI tells it apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailKind {
    /// The backend changes nothing ([`Capabilities::read_only`]).
    ReadOnly,
    Network,
    /// Signed out, or the sign-in is no longer accepted.
    Auth,
    /// Slack asked to slow down, for `retry_after` when it said how long.
    RateLimited {
        retry_after: Option<Duration>,
    },
    /// The backend's own limit of requests is reached.
    CapReached,
    /// No connection, or the backend is gone.
    Offline,
    /// The backend has more requests waiting than it takes: nothing was sent; try again.
    Busy,
    /// Slack answered with an error of its own (`code`, such as `channel_not_found`).
    Api {
        code: String,
    },
}

/// An event with the generation of the request it answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    pub generation: Generation,
    pub event: Event,
}

impl Envelope {
    /// Request `request` failed.
    pub fn failed(request: Generation, kind: FailKind, detail: impl Into<String>) -> Self {
        Self { generation: request, event: Event::Failed { request, kind, detail: detail.into() } }
    }
}

/// What a read-only backend answers instead of carrying out `request`: a change is refused before
/// anything is sent anywhere. `None` for a request that changes nothing.
pub fn refuse_change(request: &Request) -> Option<Envelope> {
    request.command.changes().then(|| Envelope::failed(request.id, FailKind::ReadOnly, ""))
}

/// A source of workspace data that answers in turn (the demo, tests): the binary runs it behind
/// a [`BackendHandle`].
pub trait Backend {
    fn capabilities(&self) -> Capabilities;

    /// Start `request`; its events carry its id.
    fn send(&mut self, request: Request);

    /// The next event that is ready, if any (never blocks).
    fn poll(&mut self) -> Option<Envelope>;
}

/// The UI's end of a backend running on its own: requests go out and events come back over
/// bounded channels, so an event can arrive at any time and wake the loop.
#[derive(Debug)]
pub struct BackendHandle {
    pub capabilities: Capabilities,
    pub tx: mpsc::Sender<Request>,
    pub rx: mpsc::Receiver<Envelope>,
}

/// The backend's end of a [`BackendHandle`].
#[derive(Debug)]
pub struct BackendEnd {
    pub rx: mpsc::Receiver<Request>,
    pub tx: mpsc::Sender<Envelope>,
}

impl BackendHandle {
    /// A handle and the backend's end of it, each channel holding up to `bound` items.
    pub fn pair(capabilities: Capabilities, bound: usize) -> (Self, BackendEnd) {
        let (req_tx, req_rx) = mpsc::channel(bound);
        let (ev_tx, ev_rx) = mpsc::channel(bound);
        (Self { capabilities, tx: req_tx, rx: ev_rx }, BackendEnd { rx: req_rx, tx: ev_tx })
    }

    /// Hand `request` to the backend without waiting. A change to a read-only backend never
    /// reaches it, and a backend that is gone or has no room answers here and now: the request
    /// failed ([`Envelope::failed`]), it is never lost.
    pub fn send(&self, request: Request) -> Result<(), Envelope> {
        if self.capabilities.read_only
            && let Some(refused) = refuse_change(&request)
        {
            return Err(refused);
        }
        let id = request.id;
        self.tx.try_send(request).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => Envelope::failed(id, FailKind::Busy, "backend queue full"),
            mpsc::error::TrySendError::Closed(_) => Envelope::failed(id, FailKind::Offline, "backend gone"),
        })
    }
}

#[cfg(test)]
mod tests;
