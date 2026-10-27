//! The signed-in workspaces. `workspaces.toml` in the config directory names each one (team id,
//! name, host, the user's id) and holds no secret; the credentials are in the secret store under
//! the team id, as one entry. This version signs in to one workspace: signing in again replaces
//! it.
//!
//! ```toml
//! [[workspace]]
//! team = "T0123"
//! name = "Example"
//! host = "example.slack.com"
//! user = "U0123"
//! ```

use slakio_core::fault::{Fault, FaultKind};
use slakio_core::secret::{Credentials, SecretStore, Unavailable};
use std::path::Path;

/// A signed-in workspace, as `workspaces.toml` names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    /// The team id (`T…`, or `E…` for an organization): the secret store's account too.
    pub team: String,
    /// The workspace's name, as Slack gave it (remote text: shown through the sanitiser).
    pub name: String,
    /// The workspace's host (`example.slack.com`).
    pub host: String,
    /// The signed-in user's id.
    pub user: String,
}

/// A team id: letters and digits only (it names a keychain entry).
pub fn valid_team(team: &str) -> bool {
    !team.is_empty() && team.len() <= 32 && team.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The workspaces in the file at `path`; none when it does not exist. A file that cannot be read
/// or understood is a fault, never "none" (it is not written over).
pub fn load(path: &Path) -> Result<Vec<Account>, Fault> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Fault::io_at(&e, path)),
    };
    let doc = text.parse::<toml_edit::DocumentMut>().map_err(|e| Fault::toml_edit(&text, &e))?;
    let bad =
        || Fault::new(FaultKind::Toml { line: None }, "workspaces.toml: a workspace without team, name, host or user");
    let Some(item) = doc.get("workspace") else { return Ok(Vec::new()) };
    let tables = item.as_array_of_tables().ok_or_else(bad)?;
    tables
        .iter()
        .map(|t| {
            let field = |k: &str| t.get(k).and_then(toml_edit::Item::as_str).map(str::to_string);
            let a = Account { team: field("team")?, name: field("name")?, host: field("host")?, user: field("user")? };
            valid_team(&a.team).then_some(a)
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(bad)
}

/// Write `accounts` to `path` (atomically).
pub fn save(path: &Path, accounts: &[Account]) -> Result<(), Fault> {
    let mut doc = toml_edit::DocumentMut::new();
    let mut tables = toml_edit::ArrayOfTables::new();
    for a in accounts {
        let mut t = toml_edit::Table::new();
        for (k, v) in [("team", &a.team), ("name", &a.name), ("host", &a.host), ("user", &a.user)] {
            t[k] = toml_edit::value(v.as_str());
        }
        tables.push(t);
    }
    doc["workspace"] = toml_edit::Item::ArrayOfTables(tables);
    let text = format!(
        "# The workspaces slakio is signed in to. No secret is kept here: the credentials are in\n# the OS keychain (`slakio logout` removes both).\n{doc}"
    );
    slakio_core::fsutil::atomic_write(path, text.as_bytes()).map_err(|e| Fault::io_at(&e, path))
}

/// The credentials as one secret-store entry.
pub fn encode(c: &Credentials) -> String {
    format!("v1\n{}\n{}\n{}", c.token(), c.cookie(), c.cookie_ds().unwrap_or(""))
}

/// The credentials of a secret-store entry; `None` when it is not one.
pub fn decode(secret: &str) -> Option<Credentials> {
    let mut lines = secret.split('\n');
    if lines.next()? != "v1" {
        return None;
    }
    let (token, cookie, ds) = (lines.next()?, lines.next()?, lines.next()?);
    if token.is_empty() || cookie.is_empty() || lines.next().is_some() {
        return None;
    }
    let c = Credentials::new(token, cookie);
    Some(if ds.is_empty() { c } else { c.with_ds(ds) })
}

/// Keep `account` signed in with `credentials`: the secret first, then the file (which then
/// names this workspace only). A store that fails keeps nothing.
pub fn keep(store: &dyn SecretStore, path: &Path, account: &Account, credentials: &Credentials) -> Result<(), Fault> {
    if !valid_team(&account.team) {
        return Err(Fault::other("not a team id"));
    }
    store.set(&account.team, &encode(credentials)).map_err(|Unavailable(f)| f)?;
    save(path, std::slice::from_ref(account))
}

/// The credentials of `account`: `Ok(None)` when the store has none; a damaged entry is a
/// fault, not "none".
pub fn credentials(store: &dyn SecretStore, account: &Account) -> Result<Option<Credentials>, Fault> {
    match store.get(&account.team).map_err(|Unavailable(f)| f)? {
        None => Ok(None),
        Some(secret) => {
            decode(&secret).map(Some).ok_or_else(|| Fault::new(FaultKind::SecretStore, "the keychain entry is damaged"))
        }
    }
}

/// Sign out of every workspace in `path`: their secrets are removed, then the file. The
/// workspaces signed out of.
pub fn forget(store: &dyn SecretStore, path: &Path) -> Result<Vec<Account>, Fault> {
    let accounts = load(path)?;
    for a in &accounts {
        store.delete(&a.team).map_err(|Unavailable(f)| f)?;
    }
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(Fault::io_at(&e, path)),
        _ => {}
    }
    Ok(accounts)
}

#[cfg(test)]
mod tests;
