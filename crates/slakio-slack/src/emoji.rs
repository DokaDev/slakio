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
use serde::de::{Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use slakio_core::backend::Event;
use slakio_core::images::ADDRESS_MAX;
use slakio_core::images::emoji::{NAMES_MAX, resolve, shortcode};
use std::fmt;

/// `emoji.list`'s answer, as far as it is read: each name and its address or `alias:other`.
#[derive(Deserialize)]
struct Listed {
    #[serde(default)]
    emoji: Names,
}

/// The names of `emoji.list` kept as they are read: at most [`NAMES_MAX`], each a shortcode
/// with a text value of at most [`ADDRESS_MAX`] bytes; the rest is read past, not kept.
#[derive(Default)]
struct Names(Vec<(String, String)>);

impl<'de> Deserialize<'de> for Names {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Map;
        impl<'de> Visitor<'de> for Map {
            type Value = Names;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map of emoji names")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Names, A::Error> {
                let mut kept = Vec::new();
                while let Some(name) = map.next_key::<String>()? {
                    let Text(value) = map.next_value()?;
                    if let Some(value) = value.filter(|_| kept.len() < NAMES_MAX && shortcode(&name)) {
                        kept.push((name, value));
                    }
                }
                Ok(Names(kept))
            }
        }
        d.deserialize_map(Map)
    }
}

/// A value of the map: its text when it is text of at most [`ADDRESS_MAX`] bytes; anything
/// else is read past.
struct Text(Option<String>);

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Any;
        impl<'de> Visitor<'de> for Any {
            type Value = Text;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any value")
            }
            fn visit_str<E>(self, v: &str) -> Result<Text, E> {
                Ok(Text((v.len() <= ADDRESS_MAX).then(|| v.to_string())))
            }
            fn visit_bool<E>(self, _: bool) -> Result<Text, E> {
                Ok(Text(None))
            }
            fn visit_i64<E>(self, _: i64) -> Result<Text, E> {
                Ok(Text(None))
            }
            fn visit_u64<E>(self, _: u64) -> Result<Text, E> {
                Ok(Text(None))
            }
            fn visit_f64<E>(self, _: f64) -> Result<Text, E> {
                Ok(Text(None))
            }
            fn visit_unit<E>(self) -> Result<Text, E> {
                Ok(Text(None))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Text, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Text(None))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Text, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Text(None))
            }
        }
        d.deserialize_any(Any)
    }
}

/// The workspace's own emoji, by name, as [`Event::Emoji`].
pub(crate) async fn emoji(client: &Client) -> Result<Event, SlackError> {
    let listed: Listed = client.call(&Call::form(Method::EmojiList, &[]), Priority::Background).await?;
    Ok(Event::Emoji { emoji: resolve(&listed.emoji.0) })
}

#[cfg(test)]
mod tests;
