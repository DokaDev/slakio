//! The signed-in workspaces. `workspaces.toml` in the config directory names each one (team id,
//! name, host, the user's id) and holds no secret; the credentials are in the secret store under
//! the team id, as one entry per workspace. Signing in to a workspace adds it, or updates it when
//! it is already signed in; the others stay. Signing out removes one workspace, or all.
//!
//! Every change of the file holds `workspaces.lock` beside it, so two slakios signing in or out
//! at once never lose each other's line. A file that exists but cannot be read is never written
//! over: it is moved aside to `workspaces.toml.unreadable`, and signing out of all still finds
//! the team ids in it (and every entry the secret store holds).
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
use zeroize::Zeroizing;

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
/// or understood is a fault, never "none" (it is not written over). A team named twice (a hand
/// edit) counts once, as its first line says.
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
        .map(|mut all| {
            let mut seen = std::collections::HashSet::new();
            all.retain(|a| seen.insert(a.team.clone()));
            all
        })
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
pub fn encode(c: &Credentials) -> Zeroizing<String> {
    Zeroizing::new(format!("v1\n{}\n{}\n{}", c.token(), c.cookie(), c.cookie_ds().unwrap_or("")))
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

/// The lock held while the file at `path` is read, changed and written (released on drop).
fn locked(path: &Path) -> Result<std::fs::File, Fault> {
    let lock = path.with_extension("lock");
    if let Some(dir) = lock.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| Fault::io_at(&e, dir))?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock)
        .map_err(|e| Fault::io_at(&e, &lock))?;
    file.lock().map_err(|e| Fault::io_at(&e, &lock))?;
    Ok(file)
}

/// Where a file at `path` that cannot be read is moved to.
pub fn aside(path: &Path) -> std::path::PathBuf {
    path.with_extension("toml.unreadable")
}

/// Move the unreadable file at `path` aside (added to what was moved aside before), so that
/// writing a new one loses no team id.
fn move_aside(path: &Path) -> Result<(), Fault> {
    let text = std::fs::read(path).map_err(|e| Fault::io_at(&e, path))?;
    let to = aside(path);
    let mut kept = std::fs::read(&to).unwrap_or_default();
    kept.extend_from_slice(&text);
    kept.push(b'\n');
    slakio_core::fsutil::atomic_write(&to, &kept).map_err(|e| Fault::io_at(&e, &to))?;
    std::fs::remove_file(path).map_err(|e| Fault::io_at(&e, path))
}

/// The team ids a file names line by line (`team = "T…"`), however damaged the rest of it is.
fn salvaged(path: &Path) -> Vec<String> {
    let text = std::fs::read(path).unwrap_or_default();
    String::from_utf8_lossy(&text)
        .lines()
        .filter_map(|l| {
            let value = l.trim().strip_prefix("team")?.trim_start().strip_prefix('=')?.trim();
            let team = value.strip_prefix('"')?.strip_suffix('"')?;
            valid_team(team).then(|| team.to_string())
        })
        .collect()
}

/// Keep `account` signed in with `credentials`: the secret first, then the file, which names it
/// in its place (or last, when it is new) beside the workspaces signed in to already; their
/// secrets stay. A store that fails keeps nothing new. A file that cannot be read is moved aside
/// first ([`aside`]); when that fails too, nothing is kept.
pub fn keep(store: &dyn SecretStore, path: &Path, account: &Account, credentials: &Credentials) -> Result<(), Fault> {
    if !valid_team(&account.team) {
        return Err(Fault::other("not a team id"));
    }
    let _lock = locked(path)?;
    let mut accounts = match load(path) {
        Ok(a) => a,
        Err(_) => {
            move_aside(path)?;
            Vec::new()
        }
    };
    store.set(&account.team, &encode(credentials)).map_err(|Unavailable(f)| f)?;
    match accounts.iter_mut().find(|a| a.team == account.team) {
        Some(a) => a.clone_from(account),
        None => accounts.push(account.clone()),
    }
    save(path, &accounts)
}

/// Which of `accounts` `query` names: its team id, or its name or host (letters case-blind), or
/// the first part of its host (`example` for `example.slack.com`).
pub fn named<'a>(accounts: &'a [Account], query: &str) -> Vec<&'a Account> {
    let q = query.trim();
    let by = |f: &dyn Fn(&Account) -> bool| accounts.iter().filter(|a| f(a)).collect::<Vec<_>>();
    let exact = by(&|a| a.team == q);
    if !exact.is_empty() {
        return exact;
    }
    by(&|a| {
        a.name.eq_ignore_ascii_case(q)
            || a.host.eq_ignore_ascii_case(q)
            || a.host.split('.').next().is_some_and(|h| h.eq_ignore_ascii_case(q))
    })
}

/// Sign out of workspace `team` alone: its secret is removed, then its line of the file (the
/// file goes with the last one). The workspace signed out of; `None` when it was not signed in.
pub fn forget_one(store: &dyn SecretStore, path: &Path, team: &str) -> Result<Option<Account>, Fault> {
    let _lock = locked(path)?;
    let mut accounts = load(path)?;
    let Some(at) = accounts.iter().position(|a| a.team == team) else { return Ok(None) };
    store.delete(team).map_err(|Unavailable(f)| f)?;
    let gone = accounts.remove(at);
    if accounts.is_empty() {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(Fault::io_at(&e, path)),
            _ => {}
        }
    } else {
        save(path, &accounts)?;
    }
    Ok(Some(gone))
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

/// Sign out of every workspace: every secret-store entry found is removed — those `path` names,
/// those a file that cannot be read (or one moved aside) names, and every other the store holds
/// — then the files. The workspaces signed out of; one known by its team id alone is named by
/// it. When an entry cannot be removed (or the store cannot list them), the files stay.
pub fn forget(store: &dyn SecretStore, path: &Path) -> Result<Vec<Account>, Fault> {
    let _lock = locked(path)?;
    let mut accounts = load(path).unwrap_or_default();
    let listed = store.accounts().map_err(|Unavailable(f)| f);
    let mut teams = salvaged(path);
    teams.extend(salvaged(&aside(path)));
    teams.extend(listed.clone().unwrap_or_default().into_iter().filter(|t| valid_team(t)));
    for team in teams {
        if !accounts.iter().any(|a| a.team == team) {
            accounts.push(Account { name: team.clone(), team, host: String::new(), user: String::new() });
        }
    }
    for a in &accounts {
        store.delete(&a.team).map_err(|Unavailable(f)| f)?;
    }
    listed?;
    for file in [path.to_path_buf(), aside(path)] {
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(Fault::io_at(&e, &file)),
            _ => {}
        }
    }
    Ok(accounts)
}

#[cfg(test)]
mod tests;
