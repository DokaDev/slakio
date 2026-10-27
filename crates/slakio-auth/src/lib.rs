//! Signing slakio in to Slack, as the user's own browser is signed in:
//!
//! * [`curl`] — a request copied from the browser ("Copy as cURL"), parsed for the workspace's
//!   address and its credentials;
//! * [`desktop`] — the Slack desktop app's own on-disk state (macOS): its signed-in workspaces
//!   and their credentials, read with no network call (elsewhere: not supported yet);
//! * [`keychain`] — the OS keychain behind the core's `SecretStore` (macOS; elsewhere it says
//!   it is not available yet);
//! * [`account`] — the signed-in workspaces: their names and ids in `workspaces.toml` (no
//!   secret), their credentials in the secret store.
//!
//! The crate depends on `slakio-core` only, never on the Slack transport: whether a paste
//! signs in is asked of Slack by the binary, through the transport, before it is kept.

pub mod account;
pub mod curl;
pub mod desktop;
pub mod keychain;

pub use account::Account;
pub use curl::{PasteError, Pasted, parse};
pub use desktop::{DesktopError, DesktopWorkspace, Found, SafeStorage};
