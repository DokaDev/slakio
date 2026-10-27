//! The methods slakio may call, and nothing else: read-only ones only. A request is built from
//! a [`Method`], so a method that is not listed here cannot be asked for; a name that comes in
//! as text ([`Method::parse`]) is refused before anything is sent, counted or opened.
//!
//! None of these changes anything in Slack, and none lists a whole directory (`users.list`,
//! `conversations.list`): what the client knows is looked up by id, in batches.

use crate::error::SlackError;

/// A read-only method of Slack's web API, or of its edge cache (`edgeapi`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    ClientUserBoot,
    ClientCounts,
    ClientShouldReload,
    ConversationsView,
    ConversationsHistory,
    ConversationsReplies,
    ConversationsInfo,
    UsersInfo,
    UsersPrefsGet,
    UsersChannelSectionsList,
    EmojiList,
    StarsList,
    UsergroupsList,
    DndInfo,
    DndTeamInfo,
    /// The edge cache's channel lookup by id and version.
    EdgeChannelsInfo,
    /// The edge cache's people lookup by id and version.
    EdgeUsersInfo,
}

impl Method {
    /// Every allowed method.
    pub const ALL: &[Method] = &[
        Method::ClientUserBoot,
        Method::ClientCounts,
        Method::ClientShouldReload,
        Method::ConversationsView,
        Method::ConversationsHistory,
        Method::ConversationsReplies,
        Method::ConversationsInfo,
        Method::UsersInfo,
        Method::UsersPrefsGet,
        Method::UsersChannelSectionsList,
        Method::EmojiList,
        Method::StarsList,
        Method::UsergroupsList,
        Method::DndInfo,
        Method::DndTeamInfo,
        Method::EdgeChannelsInfo,
        Method::EdgeUsersInfo,
    ];

    /// The method's name: the last part of its URL (`client.userBoot`, `channels/info`).
    pub fn name(self) -> &'static str {
        match self {
            Method::ClientUserBoot => "client.userBoot",
            Method::ClientCounts => "client.counts",
            Method::ClientShouldReload => "client.shouldReload",
            Method::ConversationsView => "conversations.view",
            Method::ConversationsHistory => "conversations.history",
            Method::ConversationsReplies => "conversations.replies",
            Method::ConversationsInfo => "conversations.info",
            Method::UsersInfo => "users.info",
            Method::UsersPrefsGet => "users.prefs.get",
            Method::UsersChannelSectionsList => "users.channelSections.list",
            Method::EmojiList => "emoji.list",
            Method::StarsList => "stars.list",
            Method::UsergroupsList => "usergroups.list",
            Method::DndInfo => "dnd.info",
            Method::DndTeamInfo => "dnd.teamInfo",
            Method::EdgeChannelsInfo => "channels/info",
            Method::EdgeUsersInfo => "users/info",
        }
    }

    /// The method is on the edge cache, not the workspace API.
    pub fn edge(self) -> bool {
        matches!(self, Method::EdgeChannelsInfo | Method::EdgeUsersInfo)
    }

    /// The allowed method named `name`; any other name is refused, before any I/O.
    pub fn parse(name: &str) -> Result<Method, SlackError> {
        Method::ALL
            .iter()
            .copied()
            .find(|m| m.name() == name)
            .ok_or_else(|| SlackError::NotAllowed { method: name.chars().take(64).collect() })
    }

    /// The `_x_reason` the web client sends with this method; `None` for the methods it sends
    /// none with.
    pub(crate) fn reason(self) -> Option<&'static str> {
        Some(match self {
            Method::ConversationsView | Method::EdgeChannelsInfo | Method::EdgeUsersInfo => return None,
            Method::ClientUserBoot => "initial-data",
            Method::ClientShouldReload => "boot",
            Method::ClientCounts => "fetchClientCountsOnConnect",
            Method::ConversationsHistory => "message-pane/requestHistory",
            Method::UsersPrefsGet => "fetch-frecency-prefs",
            Method::DndInfo => "fetchAndUpsertDndForUsers-getDndTimesFor:self",
            _ => "conditional-fetch-manager",
        })
    }

    /// The web client sends `_x_mode=online` with this method. Only a method that has an
    /// `_x_reason` has one.
    pub(crate) fn sends_mode(self) -> bool {
        self.reason().is_some() && !matches!(self, Method::ClientUserBoot | Method::ClientShouldReload)
    }
}

#[cfg(test)]
mod tests;
