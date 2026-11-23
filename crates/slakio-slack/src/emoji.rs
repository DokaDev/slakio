//! A workspace's own emoji: Slack's `emoji.list`, read when the UI asks (once a run, and again
//! once signed in again), in the background share of the request budget. What each name draws is
//! worked out here ([`slakio_core::images::emoji::resolve`]): its picture only when it is on the
//! emoji host, its aliases followed with a cap and a guard against cycles; any other value is
//! left out, so the UI shows `:name:` for it. The pictures themselves come one by one, when
//! drawn ([`crate::picture`]).

use crate::client::Client;
use crate::error::SlackError;
use crate::governor::Priority;
use crate::method::Method;
use crate::request::Call;
use serde::Deserialize;
use slakio_core::backend::Event;
use slakio_core::images::emoji::resolve;

/// `emoji.list`'s answer, as far as it is read: each name and its address or `alias:other`.
#[derive(Deserialize)]
struct Listed {
    #[serde(default)]
    emoji: serde_json::Map<String, serde_json::Value>,
}

/// The workspace's own emoji, by name, as [`Event::Emoji`].
pub(crate) async fn emoji(client: &Client) -> Result<Event, SlackError> {
    let listed: Listed = client.call(&Call::form(Method::EmojiList, &[]), Priority::Background).await?;
    let pairs: Vec<(String, String)> =
        listed.emoji.into_iter().filter_map(|(name, v)| v.as_str().map(|v| (name, v.to_string()))).collect();
    Ok(Event::Emoji { emoji: resolve(&pairs) })
}
