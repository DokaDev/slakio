//! Signing slakio in to Slack, as the user's own browser is signed in:
//!
//! * [`curl`] — a request copied from the browser ("Copy as cURL"), parsed for the workspace's
//!   address and its credentials;
//! * [`keychain`] — the OS keychain behind the core's `SecretStore` (macOS; elsewhere it says
//!   it is not available yet);
//! * [`account`] — the signed-in workspaces: their names and ids in `workspaces.toml` (no
//!   secret), their credentials in the secret store.
//!
//! The crate depends on `slakio-core` only, never on the Slack transport: whether a paste
//! signs in is asked of Slack by the binary, through the transport, before it is kept.

pub mod account;
pub mod curl;
pub mod keychain;

pub use account::Account;
pub use curl::{PasteError, Pasted, parse};
