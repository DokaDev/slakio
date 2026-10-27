//! The local cache of slakio: what an earlier run fetched from Slack — the last boot snapshot
//! and the newest messages of the conversations opened — in one SQLite file in the cache
//! directory, so a start draws at once, before any request, and then refreshes from Slack.
//!
//! * [`db`] — opening it: private files, one writer at a time, a damaged or newer cache set up
//!   again;
//! * [`schema`] — its tables and the migrations of older versions;
//! * [`snapshot`], [`messages`] — what is kept, and read back;
//! * [`prune`] — the caps (200 messages a conversation, 500 MB, 30 days by default), never
//!   touching a conversation shown;
//! * [`backup`] — the cache kept out of Time Machine backups (macOS).
//!
//! No secret is ever kept here: the credentials stay in the OS keychain. The crate depends on
//! `slakio-core` only.

pub mod backup;
mod codec;
pub mod db;
pub mod messages;
pub mod prune;
pub mod schema;
pub mod snapshot;

pub use backup::{Exclude, Tmutil};
pub use db::{OpenError, Opened, Store};
pub use prune::Caps;

#[cfg(test)]
mod testutil;
