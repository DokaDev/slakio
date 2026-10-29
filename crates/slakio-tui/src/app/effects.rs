//! What only the binary can do, queued by the app and done by its loop.

use super::signin::Pasted;
use slakio_core::fault::Fault;

/// Something only the binary can do, done by its loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Put text on the system clipboard (OSC 52; the terminal does it, no clipboard library
    /// is touched).
    Copy(String),
    /// Save `key = value` in the config file (an answer the app asked for).
    Save { key: &'static str, value: String },
    /// Append `fault` to the error log, as having happened while `context`.
    Record { context: &'static str, fault: Fault },
    /// Sign in with a pasted request ([`super::signin`]).
    SignIn(Pasted),
    /// Sign in with the desktop workspaces of these team ids, picked in the picker, in its order
    /// ([`super::signin`]); the binary reads their credentials (the keychain may prompt) and
    /// checks them with Slack, one after the other.
    DesktopPick(Vec<String>),
    /// Switch the network off (`false`) or on again for the rest of the run (`:net`): the binary
    /// tells the app whether it is as asked ([`super::App::net_switched`]).
    Network(bool),
}
