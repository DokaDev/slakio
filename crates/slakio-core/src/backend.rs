//! The protocol between the UI and a backend (the demo world now, Slack later): the UI sends
//! [`Command`]s and receives [`Event`]s, each tagged with the [`Generation`] of the request it
//! answers so an answer that arrives after a newer request is dropped instead of drawn.
//!
//! The UI state never calls a backend: it queues commands, and the binary's loop delivers them
//! and hands the events back. Only one wiring file of the UI names a concrete backend.

use crate::model::{Conversation, Message, Section, Target, Ts, User, Workspace};

/// Which request an event answers. The UI bumps it for every new request of the same kind and
/// drops events of an older one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Generation(pub u64);

impl Generation {
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// What a backend can do, so the UI offers only that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// The data is invented (the demo): the status line says so, and nothing reaches Slack.
    pub demo: bool,
}

/// A request to the backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Load the workspaces, people, sidebar sections and conversations.
    Boot,
    /// Up to `limit` messages of `target` older than `before` (the newest when `None`). Answered
    /// with [`Event::History`].
    History { target: Target, before: Option<Ts>, limit: u32 },
}

/// A page of messages: the answer to [`Command::History`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub target: Target,
    /// Oldest first. A thread's first page (the oldest) starts with the thread's own message.
    pub messages: Vec<Message>,
    /// Nothing older exists: the start of the conversation or thread is in this page.
    pub complete: bool,
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
}

/// Something the backend tells the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Booted(Box<Snapshot>),
    History(Box<Page>),
}

/// An event with the generation of the request it answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    pub generation: Generation,
    pub event: Event,
}

/// A source of workspace data.
pub trait Backend {
    fn capabilities(&self) -> Capabilities;

    /// Start `command`; its events carry `generation`.
    fn send(&mut self, generation: Generation, command: Command);

    /// The next event that is ready, if any (never blocks).
    fn poll(&mut self) -> Option<Envelope>;
}
