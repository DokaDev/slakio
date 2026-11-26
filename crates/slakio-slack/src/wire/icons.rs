//! A bot's message's sender as Slack gives it: its team and its picture.

use super::loose;
use serde::Deserialize;

/// The bot that posted a message: its team and its picture.
#[derive(Debug, Default, Deserialize)]
pub struct BotProfile {
    pub team_id: Option<String>,
    #[serde(default, deserialize_with = "loose")]
    pub icons: Option<Icons>,
}

/// A bot's picture in its sizes (pixels square); each field that cannot be read is none.
#[derive(Debug, Default, Deserialize)]
pub struct Icons {
    #[serde(default, deserialize_with = "loose")]
    pub image_72: Option<String>,
    #[serde(default, deserialize_with = "loose")]
    pub image_48: Option<String>,
    #[serde(default, deserialize_with = "loose")]
    pub image_132: Option<String>,
    #[serde(default, deserialize_with = "loose")]
    pub image_230: Option<String>,
    #[serde(default, deserialize_with = "loose")]
    pub image_64: Option<String>,
    #[serde(default, deserialize_with = "loose")]
    pub image_36: Option<String>,
}

impl Icons {
    /// The picture, as a profile photo's ([`super::Profile::photo`]): the first of 72 and 48 pixels,
    /// the larger ones, the smaller ones, on an avatar host
    /// ([`slakio_core::images::avatar_address`]).
    pub fn photo(&self) -> Option<String> {
        [&self.image_72, &self.image_48, &self.image_132, &self.image_230, &self.image_64, &self.image_36]
            .into_iter()
            .find_map(|i| i.as_deref().and_then(slakio_core::images::avatar_address))
    }
}
