//! What only the binary can do, queued by the app and done by its loop.

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
}
