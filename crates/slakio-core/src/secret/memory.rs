//! In-process [`SecretStore`]: every test, and development runs with
//! `SLAKIO_SECRET_STORE=memory`.

use super::{SecretStore, Unavailable};
use crate::fault::{Fault, FaultKind};
use std::collections::HashMap;
use std::sync::Mutex;

/// In-memory store. [`MemoryStore::failing`] behaves like a store that cannot be used (every
/// call fails); [`MemoryStore::unlisted`] like one that cannot list its entries (a locked
/// keychain searched without a prompt) but reads, writes and removes them.
#[derive(Default)]
pub struct MemoryStore {
    map: Mutex<HashMap<String, String>>,
    unavailable: Option<Fault>,
    unlisted: Option<Fault>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// A store that cannot be used (`reason` is the detail).
    pub fn unavailable(reason: &str) -> Self {
        Self::failing(Fault::new(FaultKind::SecretStore, reason))
    }

    /// Every call fails with `fault`.
    pub fn failing(fault: Fault) -> Self {
        Self { map: Mutex::default(), unavailable: Some(fault), unlisted: None }
    }

    /// Listing the entries fails with `fault`; every other call works.
    pub fn unlisted(fault: Fault) -> Self {
        Self { unlisted: Some(fault), ..Self::default() }
    }

    fn check(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, String>>, Unavailable> {
        if let Some(r) = &self.unavailable {
            return Err(Unavailable(r.clone()));
        }
        self.map.lock().map_err(|e| Unavailable(Fault::other(e.to_string())))
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<String>, Unavailable> {
        Ok(self.check()?.get(account).cloned())
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), Unavailable> {
        self.check()?.insert(account.to_string(), secret.to_string());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<bool, Unavailable> {
        Ok(self.check()?.remove(account).is_some())
    }
    fn accounts(&self) -> Result<Vec<String>, Unavailable> {
        if let Some(f) = &self.unlisted {
            return Err(Unavailable(f.clone()));
        }
        let mut all: Vec<String> = self.check()?.keys().cloned().collect();
        all.sort();
        Ok(all)
    }
}
