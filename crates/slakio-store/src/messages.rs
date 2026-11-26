//! The newest messages of each conversation opened: every page the backend answers is kept, and
//! a start draws a conversation's newest ones from here before its own page comes.
//!
//! A page is the truth for the span of time it covers: from its oldest message (or the start of
//! the conversation, when it is complete) to the time it was asked before (or now, for the
//! newest). What the cache held in that span and the page no longer has (deleted since) goes;
//! an edited message is replaced. A conversation's time of last use (`seen`) is what ages it out
//! ([`crate::prune`]).
//!
//! A thread is kept the same way, as a place of its own named `<conversation>:<thread>` (its
//! own message's time, as Slack spells it; a Slack id has no `:`, so no conversation is named
//! so): its pages, its time of last use and the caps are its own.

use crate::codec::{count, int, pack, uint, unpack};
use crate::db::Store;
use crate::snapshot::{put_user, user};
use rusqlite::{Row, params};
use slakio_core::backend::Page;
use slakio_core::model::{File, FileId, Message, Reaction, Target, ThreadSummary, Ts, UserId};
use slakio_core::sanitize::Remote;

fn reactions(rs: &[Reaction]) -> String {
    let parts: Vec<String> = rs
        .iter()
        .flat_map(|r| [r.name.unsanitized().to_string(), r.count.to_string(), u8::from(r.mine).to_string()])
        .collect();
    pack(&parts)
}

fn unreactions(text: &str) -> rusqlite::Result<Vec<Reaction>> {
    let damaged = || rusqlite::Error::InvalidColumnName("damaged reactions".to_string());
    let parts = unpack(text).ok_or_else(damaged)?;
    if parts.len() % 3 != 0 {
        return Err(damaged());
    }
    parts
        .chunks(3)
        .map(|c| {
            let n: u32 = c[1].parse().map_err(|_| damaged())?;
            let mine = match c[2].as_str() {
                "0" => false,
                "1" => true,
                _ => return Err(damaged()),
            };
            Ok(Reaction { name: Remote::new(c[0].clone()), count: n, mine })
        })
        .collect()
}

/// The files of a message: five parts each (id, name, kind, size or empty, address or empty).
fn files(fs: &[File]) -> String {
    let parts: Vec<String> = fs
        .iter()
        .flat_map(|f| {
            [
                f.id.as_str().to_string(),
                f.name.unsanitized().to_string(),
                f.kind.unsanitized().to_string(),
                f.size.map(|n| n.to_string()).unwrap_or_default(),
                f.download.clone().unwrap_or_default(),
            ]
        })
        .collect();
    pack(&parts)
}

/// The thumbnails of a message's files: one part each, in their order (empty for none).
fn thumbs(fs: &[File]) -> String {
    if fs.iter().all(|f| f.thumb.is_none()) {
        return String::new();
    }
    pack(&fs.iter().map(|f| f.thumb.clone().unwrap_or_default()).collect::<Vec<_>>())
}

/// The files of `text` with the thumbnails of `thumbs` (empty: none, as kept before they were).
fn unfiles(text: &str, thumbs: &str) -> rusqlite::Result<Vec<File>> {
    let damaged = || rusqlite::Error::InvalidColumnName("damaged files".to_string());
    let parts = unpack(text).ok_or_else(damaged)?;
    if parts.len() % 5 != 0 {
        return Err(damaged());
    }
    let thumbs = if thumbs.is_empty() { Vec::new() } else { unpack(thumbs).ok_or_else(damaged)? };
    if !thumbs.is_empty() && thumbs.len() != parts.len() / 5 {
        return Err(damaged());
    }
    parts
        .chunks(5)
        .enumerate()
        .map(|(i, c)| {
            let size = if c[3].is_empty() { None } else { Some(c[3].parse().map_err(|_| damaged())?) };
            Ok(File {
                id: FileId::new(c[0].clone()),
                name: Remote::new(c[1].clone()),
                kind: Remote::new(c[2].clone()),
                size,
                download: Some(c[4].clone()).filter(|d| !d.is_empty()),
                // Kept as it came; checked again, as a backend's would be.
                thumb: thumbs.get(i).and_then(|t| slakio_core::images::thumb_address(t)),
            })
        })
        .collect()
}

fn message(r: &Row<'_>) -> rusqlite::Result<Message> {
    let thread = match (r.get::<_, Option<i64>>("replies")?, r.get::<_, Option<i64>>("last_reply")?) {
        (Some(n), Some(last)) => Some(ThreadSummary { replies: count(n)?, last_reply: Ts(uint(last)?) }),
        _ => None,
    };
    Ok(Message {
        ts: Ts(uint(r.get("ts")?)?),
        user: UserId::new(r.get::<_, String>("user")?),
        text: Remote::new(r.get::<_, String>("text")?),
        thread,
        reactions: unreactions(&r.get::<_, String>("reactions")?)?,
        edited: r.get("edited")?,
        emoji_pictures: Vec::new(),
        files: unfiles(&r.get::<_, String>("files")?, &r.get::<_, String>("thumbs")?)?,
    })
}

/// Where the messages of `target` are kept: its workspace, and its conversation or the place
/// of its thread.
pub(crate) fn place(target: &Target) -> (&str, String) {
    match target {
        Target::Conversation { workspace, conversation } => (workspace.as_str(), conversation.as_str().to_string()),
        Target::Thread { workspace, conversation, thread } => (workspace.as_str(), format!("{conversation}:{thread}")),
    }
}

impl Store {
    /// Keep `page`, the answer to a request for the messages of its target older than `before`
    /// (the newest: `None`), seen at `now` (seconds since the Unix epoch). The writer only.
    pub fn put_page(&mut self, page: &Page, before: Option<Ts>, now: u64) -> rusqlite::Result<()> {
        let (ws, conv) = place(&page.target);
        if self.read_only() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        let from = if page.complete { Some(0) } else { page.messages.iter().map(|m| m.ts.0).min() };
        if let Some(from) = from {
            let to = before.map_or(Ok(i64::MAX), |b| int(b.0))?;
            tx.execute(
                "DELETE FROM messages WHERE workspace = ?1 AND conversation = ?2 AND ts >= ?3 AND ts < ?4",
                params![ws, conv, int(from)?, to],
            )?;
        }
        for m in &page.messages {
            tx.execute(
                "INSERT OR REPLACE INTO messages (workspace, conversation, ts, user, text, replies, last_reply, reactions, edited,
                     files, thumbs)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    ws,
                    conv,
                    int(m.ts.0)?,
                    m.user.as_str(),
                    m.text.unsanitized(),
                    m.thread.map(|t| t.replies),
                    m.thread.map(|t| int(t.last_reply.0)).transpose()?,
                    reactions(&m.reactions),
                    m.edited,
                    files(&m.files),
                    thumbs(&m.files)
                ],
            )?;
        }
        // Only the target's workspace's people: a page never writes another workspace's part.
        for u in page.users.iter().filter(|u| u.workspace.as_str() == ws) {
            put_user(&tx, u, None)?;
        }
        tx.execute(
            "INSERT INTO seen (workspace, conversation, at) VALUES (?1, ?2, ?3)
             ON CONFLICT (workspace, conversation) DO UPDATE SET at = MAX(at, ?3)",
            params![ws, conv, int(now)?],
        )?;
        tx.commit()
    }

    /// The newest `limit` messages kept for `target` (oldest first), with the people they name
    /// whom the cache knows; `None` when it keeps none. Never `complete`: the backend's own page
    /// says whether older ones exist.
    pub fn newest_page(&self, target: &Target, limit: u32) -> rusqlite::Result<Option<Page>> {
        let (ws, conv) = place(target);
        // One read transaction: the messages and their people as one commit left them.
        let tx = self.conn.unchecked_transaction()?;
        let mut messages: Vec<Message> = tx
            .prepare("SELECT * FROM messages WHERE workspace = ?1 AND conversation = ?2 ORDER BY ts DESC LIMIT ?3")?
            .query_map(params![ws, conv, limit], message)?
            .collect::<Result<_, _>>()?;
        if messages.is_empty() {
            return Ok(None);
        }
        messages.reverse();
        let mut users = tx
            .prepare(
                "SELECT * FROM users WHERE workspace = ?1 AND id IN
                     (SELECT DISTINCT user FROM messages WHERE workspace = ?1 AND conversation = ?2) ORDER BY id",
            )?
            .query_map(params![ws, conv], user)?
            .collect::<Result<Vec<_>, _>>()?;
        tx.commit()?;
        users.retain(|u| messages.iter().any(|m| m.user == u.id));
        Ok(Some(Page { target: target.clone(), messages, complete: false, users, cut: false }))
    }
}

#[cfg(test)]
mod tests;
