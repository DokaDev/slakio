//! The OS keychain as a [`SecretStore`]: on macOS the login keychain's generic passwords, under
//! the service [`SERVICE`] with the workspace's team id as the account. macOS asks the user
//! before a new build of the app reads an entry ("Always Allow" keeps the answer for that
//! build). Other systems have no store yet: every call fails, never "no entry".
//!
//! A failure's detail is fixed words, never the store's own text (which could quote what it
//! read).

use slakio_core::fault::{Fault, FaultKind};
use slakio_core::secret::{SecretStore, Unavailable};

/// The keychain service of slakio's entries.
pub const SERVICE: &str = "slakio";

/// The OS keychain.
#[derive(Debug, Default)]
pub struct Keychain;

fn unavailable(why: &str) -> Unavailable {
    Unavailable(Fault::new(FaultKind::SecretStore, why))
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{SERVICE, unavailable};
    use keyring_core::Error;
    use keyring_core::api::CredentialStoreApi;
    use slakio_core::secret::Unavailable;

    fn why(e: &Error) -> Unavailable {
        unavailable(match e {
            Error::NoStorageAccess(_) => "the keychain is locked or access was denied",
            Error::BadEncoding(_) | Error::BadDataFormat(..) => "the keychain entry is damaged",
            Error::Ambiguous(_) => "the keychain holds more than one entry",
            _ => "the keychain failed",
        })
    }

    pub(super) fn entry(account: &str) -> Result<keyring_core::Entry, Unavailable> {
        let store = apple_native_keyring_store::keychain::Store::new().map_err(|e| why(&e))?;
        store.build(SERVICE, account, None).map_err(|e| why(&e))
    }

    pub(super) fn get(account: &str) -> Result<Option<String>, Unavailable> {
        match entry(account)?.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(Error::NoEntry) => Ok(None),
            Err(e) => Err(why(&e)),
        }
    }

    pub(super) fn set(account: &str, secret: &str) -> Result<(), Unavailable> {
        entry(account)?.set_password(secret).map_err(|e| why(&e))
    }

    pub(super) fn delete(account: &str) -> Result<bool, Unavailable> {
        match entry(account)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(Error::NoEntry) => Ok(false),
            Err(e) => Err(why(&e)),
        }
    }

    /// The accounts of slakio's entries: a search by service that reads no secret.
    pub(super) fn accounts() -> Result<Vec<String>, Unavailable> {
        let store = apple_native_keyring_store::keychain::Store::new().map_err(|e| why(&e))?;
        let spec = std::collections::HashMap::from([("service", SERVICE)]);
        let found = store.search(&spec).map_err(|e| why(&e))?;
        Ok(found
            .iter()
            .filter_map(keyring_core::Entry::get_specifiers)
            .filter(|(s, _)| s == SERVICE)
            .map(|(_, a)| a)
            .collect())
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::unavailable;
    use slakio_core::secret::Unavailable;

    const NONE: &str = "no keychain on this system yet";

    pub(super) fn get(_: &str) -> Result<Option<String>, Unavailable> {
        Err(unavailable(NONE))
    }
    pub(super) fn set(_: &str, _: &str) -> Result<(), Unavailable> {
        Err(unavailable(NONE))
    }
    pub(super) fn delete(_: &str) -> Result<bool, Unavailable> {
        Err(unavailable(NONE))
    }
    pub(super) fn accounts() -> Result<Vec<String>, Unavailable> {
        Err(unavailable(NONE))
    }
}

impl SecretStore for Keychain {
    fn get(&self, account: &str) -> Result<Option<String>, Unavailable> {
        imp::get(account)
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), Unavailable> {
        imp::set(account, secret)
    }
    fn delete(&self, account: &str) -> Result<bool, Unavailable> {
        imp::delete(account)
    }
    fn accounts(&self) -> Result<Vec<String>, Unavailable> {
        imp::accounts()
    }
}
