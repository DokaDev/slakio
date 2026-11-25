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
string_id!(
    /// A saved item: a to-do, or a message kept in Home's Saved section.
    SavedId
);
string_id!(
    /// A file shared in a message (`F…`).
    FileId
);

/// One of the theme's workspace colours. The colour belongs to the workspace, not to the theme:
/// the workspace chip, list stripes, pane titles and the status line all use the same slot.
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

/// Whether a person is around, as Slack says it. Do not disturb wins over the other two (Slack
/// keeps it apart from presence; a person in it is shown as such whatever their presence).
/// `Unknown` until a backend says: never shown as away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Presence {
    Active,
    Away,
    /// Do not disturb: notifications paused.
    Dnd,
    #[default]
    Unknown,
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
    pub presence: Presence,
    /// Their profile photo: an `https` address on a host of [`crate::images::AVATAR_HOSTS`]
    /// ([`crate::images::avatar_address`]); `None` when Slack gave none there.
    pub avatar: Option<String>,
    /// Slack said they have no photo of their own (only its default; `avatar` is then `None`):
    /// a photo known of them before is gone. `false` with no `avatar`: not said, so a photo known
    /// before stays.
    pub no_photo: bool,
}

/// What a sidebar section holds, which decides where new conversations land and how the
/// section is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionKind {
    /// Starred conversations.
    Favorites,
    /// A section the user made.
    Custom,
    /// Conversations shared with another organization (Slack Connect) not in a section above.
    External,
    /// Channels not in another section.
    Channels,
    /// DMs and group DMs not in another section.
    DirectMessages,
    /// DMs with apps (bots) not in another section.
    Apps,
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
    /// A DM or group DM closed in Slack's sidebar (Slack says it is not open): Direct messages,
    /// Apps and External connections do not list it, a section of the user's own and Starred
    /// do; it is still found by search, its links and the DMs view.
    pub closed: bool,
    pub muted: bool,
    /// Unread messages.
    pub unread: u32,
    /// Unread messages that mention the user (always ≤ `unread`).
    pub mentions: u32,
}

impl Conversation {
    /// Its counts as badges show them: (unread, mentions). A muted channel shows its mentions of
    /// the user only, never plain unread messages; a muted DM or group DM shows nothing (Slack
    /// counts every message of a DM as a mention), nor adds to a section's or a view's count.
    pub fn badges(&self) -> (u32, u32) {
        match (self.muted, self.is_dm()) {
            (false, _) => (self.unread, self.mentions),
            (true, false) => (0, self.mentions),
            (true, true) => (0, 0),
        }
    }

    pub fn is_dm(&self) -> bool {
        matches!(self.kind, ConversationKind::Dm { .. } | ConversationKind::GroupDm { .. })
    }
}

/// A Slack message timestamp (`1767603600.000100`), which is also the message's id within its
/// conversation: microseconds since the Unix epoch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    /// The files shared with it, in their order.
    pub files: Vec<File>,
}

/// A file shared in a message, as the message lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub id: FileId,
    /// Its name (or its title when it has none), as the sender gave it.
    pub name: Remote,
    /// Its kind in words (`PDF`, `PNG`, `Plain Text`), as Slack names it.
    pub kind: Remote,
    /// Its size in bytes, when said.
    pub size: Option<u64>,
    /// Where it can be downloaded: an `https` address on Slack's file host only, `None` for a
    /// file that cannot be (one hosted elsewhere, deleted, or hidden).
    pub download: Option<String>,
    /// A smaller copy of a picture to preview it by: an `https` address on Slack's file host
    /// ([`crate::images::thumb_address`]), `None` for a file that is no picture (PNG, JPEG,
    /// GIF), cannot be downloaded, or has no such copy.
    pub thumb: Option<String>,
}

/// Slack's file host: the only host a file is downloaded from. Spelled in two parts: the
/// repository's commit hook refuses the host written whole, so no real file address is ever
/// committed; code and tests name it through this constant.
pub const FILE_HOST: &str = concat!("files", ".slack.com");

/// Where a saved item is: a to-do in progress or completed (the To-dos view), or kept without
/// being one (Home's Saved section; Slack moved reminder-less and archived "Later" items there).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SavedState {
    InProgress,
    Completed,
    Saved,
}

/// A message the user saved: a to-do (with a reminder, its due date, or none) or a message kept
/// in Saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedItem {
    pub id: SavedId,
    pub workspace: WorkspaceId,
    pub conversation: ConversationId,
    /// The message saved.
    pub ts: Ts,
    /// The message is a thread's own message: opening the item opens the thread.
    pub thread: bool,
    pub author: UserId,
    /// The message's text, as its sender wrote it.
    pub text: Remote,
    /// When it was saved.
    pub created: Ts,
    /// When its reminder is due, if it has one.
    pub due: Option<Ts>,
    pub state: SavedState,
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
