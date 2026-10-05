//! Secret storage: where the credentials of a workspace are kept.
//!
//! Credentials never go into the config file. [`SecretStore`] is the interface every store
//! implements; [`MemoryStore`] is the in-process store used by every test and by development
//! runs. The OS keychain store comes with authentication.
//!
//! Which store a process may touch is decided once, at startup, from [`STORE_ENV`]
//! ([`StoreKind::from_env`]): `memory` never touches the OS keychain, so a development run
//! cannot read or change the user's real credentials, and an unknown value is an error (the
//! binary exits with code 2), because a typo must not silently reach the real keychain. A
//! process never falls back from one store to another on its own.

mod memory;

pub use memory::MemoryStore;

use crate::fault::Fault;

/// Environment variable that picks the store of the whole process: `keychain` (the default)
/// or `memory`.
pub const STORE_ENV: &str = "SLAKIO_SECRET_STORE";

/// Which [`SecretStore`] the process uses ([`STORE_ENV`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreKind {
    /// The OS credential store.
    Keychain,
    /// An in-process store that starts empty ([`MemoryStore`]).
    Memory,
}

impl StoreKind {
    /// Read the value of [`STORE_ENV`]. Unset or empty means [`StoreKind::Keychain`]; any
    /// other unknown value is an error (a typo must not silently reach the real keychain).
    pub fn from_env(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim) {
            None | Some("") => Ok(StoreKind::Keychain),
            Some(v) if v.eq_ignore_ascii_case("keychain") => Ok(StoreKind::Keychain),
            Some(v) if v.eq_ignore_ascii_case("memory") => Ok(StoreKind::Memory),
            Some(v) => Err(format!("{STORE_ENV}={v:?}: expected \"keychain\" or \"memory\"")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            StoreKind::Keychain => "keychain",
            StoreKind::Memory => "memory",
        }
    }
}

/// A store failed (why, see [`Fault`]). Unknown, never "no entry".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unavailable(pub Fault);

pub trait SecretStore: Send + Sync {
    /// `Ok(None)` when there is no entry for `account`.
    fn get(&self, account: &str) -> Result<Option<String>, Unavailable>;
    fn set(&self, account: &str, secret: &str) -> Result<(), Unavailable>;
    /// Remove the entry. `Ok(true)` when an entry was removed; deleting a missing entry is not
    /// an error (`Ok(false)`).
    fn delete(&self, account: &str) -> Result<bool, Unavailable>;
}

#[cfg(test)]
mod tests;
