//! What Activity lists: the user's notices (mentions of them, of a group of theirs, of everyone
//! in a conversation; new replies in threads they follow; reactions to their messages; DMs),
//! newest first, a page at a time ([`Feed`], the answer to [`super::Command::Activity`]).

use crate::model::{ConversationId, Ts, User, UserId, WorkspaceId};
use crate::sanitize::Remote;

/// Notices on one page of Activity at most.
pub const ACTIVITY_PAGE: u32 = 20;

/// The most characters of a cursor kept (Slack's, opaque): a longer one is no cursor.
pub const CURSOR_MAX: usize = 512;

/// What a notice is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoticeKind {
    /// The user was mentioned by name.
    Mention,
    /// Everyone in the conversation was (`@here`, `@channel`, `@everyone`).
    Everyone,
    /// A group the user is in was.
    Group,
    /// A word the user follows was written.
    Keyword,
    /// New replies in a thread the user follows (the newest of them).
    Thread,
    /// Someone reacted to the user's message.
    Reaction,
    /// A direct message.
    Dm,
}

impl NoticeKind {
    /// Every kind, in the order the docs list them.
    pub const ALL: &'static [NoticeKind] = &[
        NoticeKind::Mention,
        NoticeKind::Everyone,
        NoticeKind::Group,
        NoticeKind::Keyword,
        NoticeKind::Thread,
        NoticeKind::Reaction,
        NoticeKind::Dm,
    ];

    /// A mention of some kind (what the Mentions filter keeps).
    pub fn is_mention(self) -> bool {
        matches!(self, NoticeKind::Mention | NoticeKind::Everyone | NoticeKind::Group | NoticeKind::Keyword)
    }
}

/// One notice of Activity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// Slack's own id of the notice (unique in its workspace), never shown.
    pub key: String,
    pub kind: NoticeKind,
    /// When it happened (what Activity is ordered by).
    pub at: Ts,
    pub conversation: ConversationId,
    /// The message it is about: the one that mentions the user, the newest reply of a thread,
    /// the message reacted to.
    pub ts: Ts,
    /// The message it is a reply under, when it is a reply.
    pub thread: Option<Ts>,
    /// Who wrote the message, or (a reaction) who reacted; empty when Slack did not say.
    pub user: UserId,
    /// The message's text; `None` when it could not be read (unknown, not empty).
    pub text: Option<Remote>,
    /// The emoji name of a reaction, without colons.
    pub reaction: Option<Remote>,
    /// Slack counts it as not read yet.
    pub unread: bool,
}

/// A page of Activity: the answer to [`super::Command::Activity`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feed {
    pub workspace: WorkspaceId,
    /// The cursor the page was asked from; `None` for the first (newest) page.
    pub after: Option<String>,
    /// Newest first.
    pub notices: Vec<Notice>,
    /// Where the next (older) page starts; `None` when this is the last.
    pub next: Option<String>,
    /// People the notices name whom the backend learned of with this page.
    pub users: Vec<User>,
}
