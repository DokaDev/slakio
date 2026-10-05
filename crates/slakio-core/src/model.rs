//! The domain model every backend produces and the UI reads: workspaces, people, sidebar
//! sections, conversations and messages. Plain data with no behaviour beyond small queries, so
//! the demo world, the Slack adapter and the tests all build the same values.
//!
//! Every text a remote party chose — names, message bodies, reaction names — is [`Remote`]:
//! it reaches the screen only through the sanitiser.
//!
//! Ids are Slack's string ids (`T…`, `U…`, `C…`, `D…`), kept as opaque strings: a saved layout
//! names its targets by them, so it restores with whichever backend produced them.

use crate::sanitize::Remote;
use std::fmt;

macro_rules! string_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id!(
    /// A workspace (Slack team).
    WorkspaceId
);
string_id!(
    /// A person or bot, unique across workspaces (a Slack Connect peer keeps the id of their
    /// own organization).
    UserId
);
string_id!(
    /// A channel, DM or group DM.
    ConversationId
);
string_id!(
    /// A sidebar section of one workspace.
    SectionId
);

/// One of the theme's workspace colours. The colour belongs to the workspace, not to the theme:
/// the rail, list stripes, pane titles and the status line all use the same slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorkspaceColor(pub u8);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: Remote,
    pub color: WorkspaceColor,
    /// The user's own account in it.
    pub me: UserId,
}

/// Which organization a person belongs to, seen from the workspace that lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Org {
    /// The workspace's own organization.
    Own,
    /// Another organization (a Slack Connect peer); its name.
    External(Remote),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: UserId,
    /// The workspace that lists this person.
    pub workspace: WorkspaceId,
    /// The handle (`minsu.kim`).
    pub name: Remote,
    /// The name people see (`Minsu Kim`).
    pub display_name: Remote,
    pub org: Org,
    pub bot: bool,
}

/// What a sidebar section holds, which decides where new conversations land and how the
/// section is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionKind {
    /// Starred conversations.
    Favorites,
    /// A section the user made.
    Custom,
    /// Channels not in another section.
    Channels,
    /// DMs and group DMs not in another section.
    DirectMessages,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub id: SectionId,
    pub workspace: WorkspaceId,
    pub name: Remote,
    pub kind: SectionKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConversationKind {
    Channel { private: bool },
    Dm { user: UserId },
    GroupDm { users: Vec<UserId> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    pub id: ConversationId,
    pub workspace: WorkspaceId,
    pub kind: ConversationKind,
    /// The channel name without `#`; for a DM, the peer's display name.
    pub name: Remote,
    /// The sidebar section it is listed in.
    pub section: SectionId,
    /// Shared with another organization (Slack Connect).
    pub external: bool,
    pub muted: bool,
    /// Unread messages.
    pub unread: u32,
    /// Unread messages that mention the user (always ≤ `unread`).
    pub mentions: u32,
}

impl Conversation {
    pub fn is_dm(&self) -> bool {
        matches!(self.kind, ConversationKind::Dm { .. } | ConversationKind::GroupDm { .. })
    }
}

/// A Slack message timestamp (`1767603600.000100`), which is also the message's id within its
/// conversation: microseconds since the Unix epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ts(pub u64);

impl Ts {
    pub fn secs(self) -> u64 {
        self.0 / 1_000_000
    }
}

/// Slack's spelling: seconds, a dot and six digits.
impl fmt::Display for Ts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:06}", self.0 / 1_000_000, self.0 % 1_000_000)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reaction {
    /// The emoji name without colons (`+1`, `eyes`).
    pub name: Remote,
    pub count: u32,
    /// The user is among those who reacted.
    pub mine: bool,
}

/// The replies of a thread, as its parent message shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadSummary {
    pub replies: u32,
    pub last_reply: Ts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub ts: Ts,
    pub user: UserId,
    /// The text as the sender wrote it.
    pub text: Remote,
    pub thread: Option<ThreadSummary>,
    pub reactions: Vec<Reaction>,
    pub edited: bool,
}

/// What a pane shows, named by ids only so a saved layout restores with any backend.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Conversation {
        workspace: WorkspaceId,
        conversation: ConversationId,
    },
    /// The thread under message `thread` of a conversation: that message, then its replies.
    Thread {
        workspace: WorkspaceId,
        conversation: ConversationId,
        thread: Ts,
    },
}

impl Target {
    pub fn workspace(&self) -> &WorkspaceId {
        match self {
            Target::Conversation { workspace, .. } | Target::Thread { workspace, .. } => workspace,
        }
    }

    pub fn conversation(&self) -> &ConversationId {
        match self {
            Target::Conversation { conversation, .. } | Target::Thread { conversation, .. } => conversation,
        }
    }
}

#[cfg(test)]
mod tests;
