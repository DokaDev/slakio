//! Keeping the cache small: at most so many messages a conversation, conversations not opened
//! for so many days dropped, and the whole file under a size — dropping the conversations used
//! longest ago first. A conversation in `protect` (shown in a pane, or asked for during this run)
//! is never touched by any of the three.

use crate::codec::int;
use crate::db::Store;
use rusqlite::params;
use slakio_core::model::Target;
use std::collections::HashSet;

/// The cache's limits. The defaults: 200 messages a conversation, 500 MB, 30 days.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// The most messages kept for one conversation (the newest).
    pub per_conversation: u32,
    /// The most bytes the database may use.
    pub max_bytes: u64,
    /// A conversation not opened for this many days is dropped.
    pub max_days: u32,
}

impl Default for Caps {
    fn default() -> Self {
        Self { per_conversation: 200, max_bytes: 500 * 1024 * 1024, max_days: 30 }
    }
}

/// What a prune removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pruned {
    /// Conversations dropped whole (too old, or to fit the size).
    pub conversations: usize,
    /// Messages removed, in all.
    pub messages: usize,
}

/// `(workspace, conversation)` of a conversation target.
fn key(t: &Target) -> Option<(String, String)> {
    match t {
        Target::Conversation { workspace, conversation } => Some((workspace.0.clone(), conversation.0.clone())),
        Target::Thread { .. } => None,
    }
}

/// The bytes the database of `conn` uses, as its transaction sees them.
fn used_bytes(conn: &rusqlite::Connection) -> rusqlite::Result<u64> {
    let p = |name: &str| conn.query_row(&format!("PRAGMA {name}"), [], |r| r.get::<_, i64>(0));
    let used = (p("page_count")? - p("freelist_count")?).max(0) * p("page_size")?;
    Ok(u64::try_from(used).unwrap_or(0))
}

impl Store {
    /// The bytes the database uses (its pages in use, free ones not counted).
    pub fn used_bytes(&self) -> rusqlite::Result<u64> {
        used_bytes(&self.conn)
    }

    /// Give at most `pages` of the pages a prune freed back to the file system: `true` while
    /// more are left (and the step gave some back). The writer only.
    pub fn vacuum(&self, pages: u32) -> rusqlite::Result<bool> {
        if self.read_only() {
            return Ok(false);
        }
        let free = || self.conn.query_row("PRAGMA freelist_count", [], |r| r.get::<_, i64>(0));
        let before = free()?;
        // SQLite gives one page back each time the statement is stepped: every step taken.
        let mut st = self.conn.prepare(&format!("PRAGMA incremental_vacuum({pages})"))?;
        let mut rows = st.query([])?;
        while rows.next()?.is_some() {}
        drop(rows);
        drop(st);
        let after = free()?;
        Ok(after > 0 && after < before)
    }

    /// Apply `caps` at `now` (seconds since the Unix epoch), never touching a conversation of
    /// `protect`, in one transaction. The freed pages stay in the file until
    /// [`Store::vacuum`]. The writer only.
    pub fn prune(&mut self, caps: Caps, protect: &HashSet<Target>, now: u64) -> rusqlite::Result<Pruned> {
        if self.read_only() {
            return Ok(Pruned::default());
        }
        let protect: HashSet<(String, String)> = protect.iter().filter_map(key).collect();
        let kept = |k: &(String, String)| protect.contains(k);
        let mut out = Pruned::default();
        // Every conversation the cache has messages of, used longest ago first (never seen: 0).
        let convs = |conn: &rusqlite::Connection| -> rusqlite::Result<Vec<((String, String), i64)>> {
            let mut st = conn.prepare(
                "SELECT m.workspace, m.conversation, COALESCE(s.at, 0) AS at FROM messages m
                 LEFT JOIN seen s ON s.workspace = m.workspace AND s.conversation = m.conversation
                 GROUP BY m.workspace, m.conversation ORDER BY at, m.workspace, m.conversation",
            )?;
            st.query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))?.collect()
        };
        let tx = self.conn.transaction()?;
        let drop_conv = |k: &(String, String)| -> rusqlite::Result<usize> {
            let n = tx.execute("DELETE FROM messages WHERE workspace = ?1 AND conversation = ?2", params![k.0, k.1])?;
            tx.execute("DELETE FROM seen WHERE workspace = ?1 AND conversation = ?2", params![k.0, k.1])?;
            Ok(n)
        };
        let oldest = int(now.saturating_sub(u64::from(caps.max_days) * 86_400))?;
        let mut st = tx.prepare(
            "SELECT workspace, conversation FROM seen WHERE at < ?1
             UNION SELECT DISTINCT workspace, conversation FROM messages m WHERE NOT EXISTS
                 (SELECT 1 FROM seen s WHERE s.workspace = m.workspace AND s.conversation = m.conversation)",
        )?;
        let stale: Vec<(String, String)> =
            st.query_map([oldest], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        drop(st);
        for k in stale.iter().filter(|k| !kept(k)) {
            out.messages += drop_conv(k)?;
            out.conversations += 1;
        }
        let mut st = tx.prepare(
            "SELECT workspace, conversation FROM messages GROUP BY workspace, conversation HAVING COUNT(*) > ?1",
        )?;
        let long: Vec<(String, String)> =
            st.query_map([caps.per_conversation], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        drop(st);
        for k in long.iter().filter(|k| !kept(k)) {
            out.messages += tx.execute(
                "DELETE FROM messages WHERE workspace = ?1 AND conversation = ?2 AND ts NOT IN
                     (SELECT ts FROM messages WHERE workspace = ?1 AND conversation = ?2 ORDER BY ts DESC LIMIT ?3)",
                params![k.0, k.1, caps.per_conversation],
            )?;
        }
        // Over the size: whole conversations go, used longest ago first, until it fits.
        if used_bytes(&tx)? > caps.max_bytes {
            for (k, _) in convs(&tx)? {
                if kept(&k) {
                    continue;
                }
                out.messages += drop_conv(&k)?;
                out.conversations += 1;
                if used_bytes(&tx)? <= caps.max_bytes {
                    break;
                }
            }
        }
        tx.commit()?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
