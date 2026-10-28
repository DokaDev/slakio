//! The parts of Slack's answers the client reads, decoded tolerantly: unknown fields are
//! ignored, every field but an id is optional and stays `None` when missing (unknown, not
//! zero), and an entry of a list that cannot be read (no id, a wrong type) is left out instead
//! of failing the whole answer.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

/// A list whose unreadable entries are left out; `None` when the field is missing or not a list.
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<Vec<T>>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(d)?;
    Ok(match value {
        Some(serde_json::Value::Array(items)) => {
            Some(items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect())
        }
        _ => None,
    })
}

/// A value that cannot be read is `None`, not a failure of the whole answer.
fn loose<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(d)?;
    Ok(value.and_then(|v| serde_json::from_value(v).ok()))
}

/// `client.userBoot`.
#[derive(Debug, Default, Deserialize)]
pub struct UserBoot {
    #[serde(rename = "self")]
    pub me: Option<Me>,
    pub team: Option<Team>,
    #[serde(default, deserialize_with = "lenient")]
    pub channels: Option<Vec<Channel>>,
    #[serde(default, deserialize_with = "lenient")]
    pub ims: Option<Vec<Im>>,
    #[serde(default, deserialize_with = "loose")]
    pub prefs: Option<Prefs>,
    /// The user's own do-not-disturb times.
    #[serde(default, deserialize_with = "loose")]
    pub dnd: Option<Dnd>,
}

/// The user's preferences, of which the client reads the conversations muted.
#[derive(Debug, Default, Deserialize)]
pub struct Prefs {
    /// Notification preferences as JSON in a string: `{"channels": {"C1": {"muted": true}}}`.
    pub all_notifications_prefs: Option<String>,
    /// The older spelling: ids, comma-separated.
    pub muted_channels: Option<String>,
}

impl Prefs {
    /// The ids of the conversations muted, by either spelling; unreadable parts are left out.
    pub fn muted(&self) -> std::collections::HashSet<String> {
        let mut out: std::collections::HashSet<String> = self
            .muted_channels
            .as_deref()
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .collect();
        let all: Option<serde_json::Value> =
            self.all_notifications_prefs.as_deref().and_then(|s| serde_json::from_str(s).ok());
        if let Some(channels) = all.as_ref().and_then(|v| v.get("channels")).and_then(serde_json::Value::as_object) {
            for (id, prefs) in channels {
                match prefs.get("muted").and_then(serde_json::Value::as_bool) {
                    Some(true) => {
                        out.insert(id.clone());
                    }
                    // The newer spelling says it is not muted: it is not.
                    Some(false) => {
                        out.remove(id);
                    }
                    None => {}
                }
            }
        }
        out
    }
}

/// Someone's do-not-disturb times (seconds since the Unix epoch).
#[derive(Debug, Default, Deserialize)]
pub struct Dnd {
    pub dnd_enabled: Option<bool>,
    pub next_dnd_start_ts: Option<u64>,
    pub next_dnd_end_ts: Option<u64>,
    pub snooze_enabled: Option<bool>,
    pub snooze_endtime: Option<u64>,
}

impl Dnd {
    /// In do not disturb at `now` (seconds); `None` when Slack said too little to tell.
    pub fn active(&self, now: u64) -> Option<bool> {
        if self.snooze_enabled == Some(true) && self.snooze_endtime.is_some_and(|end| now < end) {
            return Some(true);
        }
        match (self.dnd_enabled?, self.next_dnd_start_ts, self.next_dnd_end_ts) {
            (false, _, _) => Some(false),
            (true, Some(start), Some(end)) => Some(start <= now && now < end),
            (true, _, _) => None,
        }
    }
}

/// `dnd.teamInfo`: people's do-not-disturb times by id.
#[derive(Debug, Default, Deserialize)]
pub struct DndTeam {
    #[serde(default, deserialize_with = "loose")]
    pub users: Option<std::collections::HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
pub struct Me {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: Option<String>,
    pub is_private: Option<bool>,
    pub is_mpim: Option<bool>,
    pub is_ext_shared: Option<bool>,
    pub is_archived: Option<bool>,
    /// Shared with another workspace of the same organization (not Slack Connect).
    pub is_org_shared: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct Im {
    pub id: String,
    pub user: Option<String>,
}

/// `client.counts`.
#[derive(Debug, Default, Deserialize)]
pub struct Counts {
    #[serde(default, deserialize_with = "lenient")]
    pub channels: Option<Vec<Count>>,
    #[serde(default, deserialize_with = "lenient")]
    pub mpims: Option<Vec<Count>>,
    #[serde(default, deserialize_with = "lenient")]
    pub ims: Option<Vec<Count>>,
}

#[derive(Debug, Deserialize)]
pub struct Count {
    pub id: String,
    pub has_unreads: Option<bool>,
    pub mention_count: Option<u32>,
}

/// `conversations.view`: the conversation's newest messages under `history`, and (some of)
/// their authors.
#[derive(Debug, Default, Deserialize)]
pub struct View {
    pub history: Option<History>,
    #[serde(default, deserialize_with = "lenient")]
    pub users: Option<Vec<Person>>,
}

/// A person, as a view and the edge cache's `users/info` describe one.
#[derive(Debug, Deserialize)]
pub struct Person {
    pub id: String,
    pub team_id: Option<String>,
    pub name: Option<String>,
    pub real_name: Option<String>,
    pub is_bot: Option<bool>,
    pub profile: Option<Profile>,
}

#[derive(Debug, Deserialize)]
pub struct Profile {
    pub display_name: Option<String>,
    pub real_name: Option<String>,
}

impl Person {
    /// The name people see: the display name, else the real name, else the handle; `None`
    /// when Slack gave none (an empty one is none).
    pub fn shown_name(&self) -> Option<&str> {
        let p = self.profile.as_ref();
        [
            p.and_then(|p| p.display_name.as_deref()),
            p.and_then(|p| p.real_name.as_deref()),
            self.real_name.as_deref(),
            self.name.as_deref(),
        ]
        .into_iter()
        .flatten()
        .find(|n| !n.trim().is_empty())
    }
}

/// The edge cache's `users/info`.
#[derive(Debug, Default, Deserialize)]
pub struct EdgeUsers {
    #[serde(default, deserialize_with = "lenient")]
    pub results: Option<Vec<Person>>,
}

/// `conversations.history` and `conversations.replies` (and `history` of a view).
#[derive(Debug, Default, Deserialize)]
pub struct History {
    #[serde(default, deserialize_with = "lenient")]
    pub messages: Option<Vec<Message>>,
    pub has_more: Option<bool>,
    #[serde(default, deserialize_with = "loose")]
    pub response_metadata: Option<Metadata>,
}

/// Where the next page starts (`conversations.replies` pages forward by cursor).
#[derive(Debug, Default, Deserialize)]
pub struct Metadata {
    pub next_cursor: Option<String>,
}

impl History {
    /// The cursor of the next page, when Slack gave one (an empty one is none).
    pub fn next_cursor(&self) -> Option<&str> {
        self.response_metadata.as_ref()?.next_cursor.as_deref().filter(|c| !c.is_empty())
    }
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub ts: String,
    pub user: Option<String>,
    pub text: Option<String>,
    pub reply_count: Option<u32>,
    pub latest_reply: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub reactions: Option<Vec<Reaction>>,
    /// Present (whatever it holds) when the message was edited.
    pub edited: Option<serde_json::Value>,
    /// `message_deleted`, `tombstone`, `bot_message`, `channel_join`, …
    pub subtype: Option<String>,
    /// Slack keeps it from the message list (a deleted message's leftover).
    pub hidden: Option<bool>,
    /// Block Kit: what bots post, and the rich text people write.
    pub blocks: Option<serde_json::Value>,
    /// Legacy attachments (bots, unfurls).
    pub attachments: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "lenient")]
    pub files: Option<Vec<File>>,
    /// A bot's message: the bot's id and the name it posts as.
    pub bot_id: Option<String>,
    pub username: Option<String>,
    /// The team of the one who posted (in a channel shared with another organization, maybe
    /// another than the workspace's).
    pub team: Option<String>,
    #[serde(default, deserialize_with = "loose")]
    pub bot_profile: Option<BotProfile>,
}

/// The bot that posted a message: its team.
#[derive(Debug, Default, Deserialize)]
pub struct BotProfile {
    pub team_id: Option<String>,
}

/// A file shared with a message: the client reads its name only.
#[derive(Debug, Deserialize)]
pub struct File {
    pub name: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Reaction {
    pub name: String,
    pub count: Option<u32>,
    #[serde(default, deserialize_with = "lenient")]
    pub users: Option<Vec<String>>,
}

/// A Slack timestamp (`1767603600.000100`) as microseconds; `None` when it is not one.
pub fn ts(s: &str) -> Option<u64> {
    let (secs, micros) = s.split_once('.').unwrap_or((s, "0"));
    if micros.len() > 6 || !micros.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let micros: u64 = format!("{micros:0<6}").parse().ok()?;
    secs.parse::<u64>().ok()?.checked_mul(1_000_000)?.checked_add(micros)
}

#[cfg(test)]
mod tests;
