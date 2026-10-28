//! The local cache of slakio: what an earlier run fetched from Slack — the last boot snapshot
//! and the newest messages of the conversations opened — in one SQLite file in the cache
//! directory, so a start draws at once, without waiting for Slack, and then refreshes from it.
//!
//! * [`db`] — opening it: private files, one writer at a time, a damaged or newer cache set up
//!   again;
//! * [`schema`] — its tables and the migrations of older versions;
//! * [`snapshot`], [`messages`] — what is kept, and read back;
//! * [`prune`] — the caps (200 messages a conversation, 500 MB, 30 days by default), never
//!   touching a conversation shown;
//! * [`backup`] — the cache kept out of Time Machine backups (macOS);
//! * [`cache`] — the store on a thread of its own; [`layer`] — the cache in front of a backend.
//!
//! No secret is ever kept here: the credentials stay in the OS keychain. The crate depends on
//! `slakio-core` only.

pub mod backup;
pub mod cache;
mod codec;
pub mod db;
pub mod layer;
pub mod messages;
pub mod prune;
pub mod schema;
pub mod snapshot;

pub use backup::{Exclude, Tmutil};
pub use cache::Cache;
pub use db::{OpenError, Opened, Store};
pub use prune::Caps;

#[cfg(test)]
mod testutil;
