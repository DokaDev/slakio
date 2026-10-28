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
    /// Every account the store holds an entry for (slakio's entries only).
    fn accounts(&self) -> Result<Vec<String>, Unavailable>;
}

/// What signs the client in to one workspace, as the web client holds it: the session token
/// (`xoxc`), the `d` cookie and, when the browser had one, the `d-s` cookie. Their text never
/// shows in a `Debug` print, so none reaches a log or a panic message by accident, and it is
/// wiped from memory when the value is dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    token: String,
    cookie: String,
    cookie_ds: Option<String>,
}

impl Credentials {
    pub fn new(token: impl Into<String>, cookie: impl Into<String>) -> Self {
        Self { token: token.into(), cookie: cookie.into(), cookie_ds: None }
    }

    /// With the `d-s` cookie's value too.
    #[must_use]
    pub fn with_ds(mut self, value: impl Into<String>) -> Self {
        self.cookie_ds = Some(value.into());
        self
    }

    /// The `d-s` cookie's value, when there is one.
    pub fn cookie_ds(&self) -> Option<&str> {
        self.cookie_ds.as_deref()
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// The `d` cookie's value.
    pub fn cookie(&self) -> &str {
        &self.cookie
    }
}

/// Wiped from memory when dropped.
impl zeroize::Zeroize for Credentials {
    fn zeroize(&mut self) {
        self.token.zeroize();
        self.cookie.zeroize();
        if let Some(ds) = self.cookie_ds.as_mut() {
            ds.zeroize();
        }
        self.cookie_ds = None;
    }
}

impl Drop for Credentials {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(self);
    }
}

impl zeroize::ZeroizeOnDrop for Credentials {}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credentials { .. }")
    }
}

#[cfg(test)]
mod tests;
