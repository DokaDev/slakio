//! The cache's tables and how a database of an older version is brought up to this one.
//!
//! `schema_version` holds one row, the number of [`MIGRATIONS`] applied. Opening applies the
//! ones after it, each with its new number in one transaction, so a migration that fails leaves
//! the database as it was. A database of a newer version (a newer slakio wrote it) is not read:
//! the cache is set up again ([`Schema::Newer`]), since it only holds what can be fetched again.
//!
//! What a remote party chose (names, message texts) is kept as it came, unsanitised: the reader
//! makes it `Remote` again, so it reaches the screen only through the sanitiser, as when it came
//! from Slack. No secret is ever kept here (the credentials are in the OS keychain).

use rusqlite::{Connection, OptionalExtension};

/// The schema, one migration per version: entry `i` takes version `i` to `i + 1`. Only ever
/// appended to; a released entry never changes.
pub const MIGRATIONS: &[&str] = &[
    // 1: the last boot snapshot, the newest messages of the conversations opened, when each was
    // last seen, and a few facts about the cache itself.
    "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
     CREATE TABLE workspaces (ord INTEGER PRIMARY KEY, id TEXT NOT NULL, name TEXT NOT NULL,
         color INTEGER NOT NULL, me TEXT NOT NULL);
     CREATE TABLE users (workspace TEXT NOT NULL, id TEXT NOT NULL,
         ord INTEGER, -- its place in the boot snapshot; NULL when only a page brought it
         name TEXT NOT NULL, display_name TEXT NOT NULL,
         org TEXT, -- NULL: the workspace's own organization; else the other one's name
         bot INTEGER NOT NULL, presence INTEGER NOT NULL,
         PRIMARY KEY (workspace, id)) WITHOUT ROWID;
     CREATE TABLE sections (ord INTEGER PRIMARY KEY, id TEXT NOT NULL, workspace TEXT NOT NULL,
         name TEXT NOT NULL, kind INTEGER NOT NULL);
     CREATE TABLE conversations (ord INTEGER PRIMARY KEY, id TEXT NOT NULL, workspace TEXT NOT NULL,
         kind INTEGER NOT NULL, private INTEGER NOT NULL, peers TEXT NOT NULL, name TEXT NOT NULL,
         section TEXT NOT NULL, external INTEGER NOT NULL, muted INTEGER NOT NULL,
         unread INTEGER NOT NULL, mentions INTEGER NOT NULL);
     CREATE TABLE saved (ord INTEGER PRIMARY KEY, id TEXT NOT NULL, workspace TEXT NOT NULL,
         conversation TEXT NOT NULL, ts INTEGER NOT NULL, thread INTEGER NOT NULL,
         author TEXT NOT NULL, text TEXT NOT NULL, created INTEGER NOT NULL, due INTEGER,
         state INTEGER NOT NULL);
     CREATE TABLE messages (workspace TEXT NOT NULL, conversation TEXT NOT NULL, ts INTEGER NOT NULL,
         user TEXT NOT NULL, text TEXT NOT NULL, replies INTEGER, last_reply INTEGER,
         reactions TEXT NOT NULL, edited INTEGER NOT NULL,
         PRIMARY KEY (workspace, conversation, ts));
     CREATE TABLE seen (workspace TEXT NOT NULL, conversation TEXT NOT NULL, at INTEGER NOT NULL,
         PRIMARY KEY (workspace, conversation)) WITHOUT ROWID;",
];

/// Where a database stands against `migrations`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Schema {
    /// Up to date (now).
    Current,
    /// Written by a newer version: not read, not changed.
    Newer(u32),
}

/// The version of the database: 0 for a new one.
pub fn version(conn: &Connection) -> rusqlite::Result<u32> {
    let table: Option<String> = conn
        .query_row("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'schema_version'", [], |r| r.get(0))
        .optional()?;
    if table.is_none() {
        return Ok(0);
    }
    conn.query_row("SELECT version FROM schema_version", [], |r| r.get(0)).optional().map(Option::unwrap_or_default)
}

/// Bring the database up to the last of `migrations`, one transaction per step.
pub fn migrate(conn: &mut Connection, migrations: &[&str]) -> rusqlite::Result<Schema> {
    let at = version(conn)?;
    let latest = u32::try_from(migrations.len()).unwrap_or(u32::MAX);
    if at > latest {
        return Ok(Schema::Newer(at));
    }
    for (step, sql) in (at..latest).zip(&migrations[at as usize..]) {
        let tx = conn.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)")?;
        tx.execute_batch(sql)?;
        tx.execute("DELETE FROM schema_version", [])?;
        tx.execute("INSERT INTO schema_version (version) VALUES (?1)", [step + 1])?;
        tx.commit()?;
    }
    Ok(Schema::Current)
}

#[cfg(test)]
mod tests;
