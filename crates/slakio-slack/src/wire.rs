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

/// `conversations.view`: the conversation's newest messages under `history`.
#[derive(Debug, Default, Deserialize)]
pub struct View {
    pub history: Option<History>,
}

/// `conversations.history` and `conversations.replies` (and `history` of a view).
#[derive(Debug, Default, Deserialize)]
pub struct History {
    #[serde(default, deserialize_with = "lenient")]
    pub messages: Option<Vec<Message>>,
    pub has_more: Option<bool>,
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
