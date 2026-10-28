//! The last boot snapshot of each workspace: written when the backend answers that workspace's
//! boot, read back at the next start. A workspace's new snapshot replaces its own part only; the
//! other workspaces' parts are never touched by it (signing out of one removes its part:
//! [`Store::forget`]).
//!
//! Every list keeps its order (`ord`); every enum is a small number, and one this version does
//! not know is damage (an error), never a guess.

use crate::codec::{count, int, pack, uint, unpack};
use crate::db::Store;
use rusqlite::{Connection, Row, Transaction, params};
use slakio_core::backend::Snapshot;
use slakio_core::model::{
    Conversation, ConversationId, ConversationKind, Org, Presence, SavedId, SavedItem, SavedState, Section, SectionId,
    SectionKind, Ts, User, UserId, Workspace, WorkspaceColor, WorkspaceId,
};
use slakio_core::sanitize::Remote;

/// A value of the database this version cannot read.
fn damaged(what: &str) -> rusqlite::Error {
    rusqlite::Error::InvalidColumnName(format!("damaged {what}"))
}

fn presence_code(p: Presence) -> i64 {
    match p {
        Presence::Active => 0,
        Presence::Away => 1,
        Presence::Dnd => 2,
        Presence::Unknown => 3,
    }
}

fn presence(n: i64) -> rusqlite::Result<Presence> {
    Ok(match n {
        0 => Presence::Active,
        1 => Presence::Away,
        2 => Presence::Dnd,
        3 => Presence::Unknown,
        _ => return Err(damaged("presence")),
    })
}

fn section_code(k: SectionKind) -> i64 {
    match k {
        SectionKind::Favorites => 0,
        SectionKind::Custom => 1,
        SectionKind::Channels => 2,
        SectionKind::DirectMessages => 3,
    }
}

fn section_kind(n: i64) -> rusqlite::Result<SectionKind> {
    Ok(match n {
        0 => SectionKind::Favorites,
        1 => SectionKind::Custom,
        2 => SectionKind::Channels,
        3 => SectionKind::DirectMessages,
        _ => return Err(damaged("section kind")),
    })
}

fn state_code(s: SavedState) -> i64 {
    match s {
        SavedState::InProgress => 0,
        SavedState::Completed => 1,
        SavedState::Saved => 2,
    }
}

fn state(n: i64) -> rusqlite::Result<SavedState> {
    Ok(match n {
        0 => SavedState::InProgress,
        1 => SavedState::Completed,
        2 => SavedState::Saved,
        _ => return Err(damaged("saved state")),
    })
}

/// A person, as a page or the snapshot brings them; `ord` is their place in the snapshot.
pub(crate) fn put_user(tx: &Transaction<'_>, u: &User, ord: Option<usize>) -> rusqlite::Result<()> {
    let org = match &u.org {
        Org::Own => None,
        Org::External(name) => Some(name.unsanitized()),
    };
    let ord = ord.map(|o| int(o as u64)).transpose()?;
    tx.execute(
        "INSERT INTO users (workspace, id, ord, name, display_name, org, bot, presence)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (workspace, id) DO UPDATE SET ord = COALESCE(?3, ord), name = ?4, display_name = ?5,
             org = ?6, bot = ?7, presence = ?8",
        params![
            u.workspace.as_str(),
            u.id.as_str(),
            ord,
            u.name.unsanitized(),
            u.display_name.unsanitized(),
            org,
            u.bot,
            presence_code(u.presence)
        ],
    )?;
    Ok(())
}

pub(crate) fn user(r: &Row<'_>) -> rusqlite::Result<User> {
    Ok(User {
        workspace: WorkspaceId::new(r.get::<_, String>("workspace")?),
        id: UserId::new(r.get::<_, String>("id")?),
        name: Remote::new(r.get::<_, String>("name")?),
        display_name: Remote::new(r.get::<_, String>("display_name")?),
        org: match r.get::<_, Option<String>>("org")? {
            None => Org::Own,
            Some(name) => Org::External(Remote::new(name)),
        },
        bot: r.get("bot")?,
        presence: presence(r.get("presence")?)?,
    })
}

fn put_conversation(tx: &Transaction<'_>, ord: usize, c: &Conversation) -> rusqlite::Result<()> {
    let (kind, private, peers): (i64, bool, Vec<&str>) = match &c.kind {
        ConversationKind::Channel { private } => (0, *private, Vec::new()),
        ConversationKind::Dm { user } => (1, false, vec![user.as_str()]),
        ConversationKind::GroupDm { users } => (2, false, users.iter().map(UserId::as_str).collect()),
    };
    tx.execute(
        "INSERT INTO conversations (ord, id, workspace, kind, private, peers, name, section, external, muted, unread, mentions)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            int(ord as u64)?,
            c.id.as_str(),
            c.workspace.as_str(),
            kind,
            private,
            pack(&peers),
            c.name.unsanitized(),
            c.section.as_str(),
            c.external,
            c.muted,
            c.unread,
            c.mentions
        ],
    )?;
    Ok(())
}

fn conversation(r: &Row<'_>) -> rusqlite::Result<Conversation> {
    let peers = unpack(&r.get::<_, String>("peers")?).ok_or_else(|| damaged("peers"))?;
    let kind = match (r.get::<_, i64>("kind")?, peers.as_slice()) {
        (0, []) => ConversationKind::Channel { private: r.get("private")? },
        (1, [user]) => ConversationKind::Dm { user: UserId::new(user.clone()) },
        (2, users) => ConversationKind::GroupDm { users: users.iter().cloned().map(UserId::new).collect() },
        _ => return Err(damaged("conversation kind")),
    };
    Ok(Conversation {
        id: ConversationId::new(r.get::<_, String>("id")?),
        workspace: WorkspaceId::new(r.get::<_, String>("workspace")?),
        kind,
        name: Remote::new(r.get::<_, String>("name")?),
        section: SectionId::new(r.get::<_, String>("section")?),
        external: r.get("external")?,
        muted: r.get("muted")?,
        unread: count(r.get("unread")?)?,
        mentions: count(r.get("mentions")?)?,
    })
}

fn put_saved(tx: &Transaction<'_>, ord: usize, s: &SavedItem) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO saved (ord, id, workspace, conversation, ts, thread, author, text, created, due, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            int(ord as u64)?,
            s.id.as_str(),
            s.workspace.as_str(),
            s.conversation.as_str(),
            int(s.ts.0)?,
            s.thread,
            s.author.as_str(),
            s.text.unsanitized(),
            int(s.created.0)?,
            s.due.map(|d| int(d.0)).transpose()?,
            state_code(s.state)
        ],
    )?;
    Ok(())
}

fn saved(r: &Row<'_>) -> rusqlite::Result<SavedItem> {
    Ok(SavedItem {
        id: SavedId::new(r.get::<_, String>("id")?),
        workspace: WorkspaceId::new(r.get::<_, String>("workspace")?),
        conversation: ConversationId::new(r.get::<_, String>("conversation")?),
        ts: Ts(uint(r.get("ts")?)?),
        thread: r.get("thread")?,
        author: UserId::new(r.get::<_, String>("author")?),
        text: Remote::new(r.get::<_, String>("text")?),
        created: Ts(uint(r.get("created")?)?),
        due: r.get::<_, Option<i64>>("due")?.map(uint).transpose()?.map(Ts),
        state: state(r.get("state")?)?,
    })
}

/// Keep workspace `ws`'s part of `s` in place of the one before; the other workspaces' parts
/// are not touched. `ord` is its place in `s`.
fn write(tx: &Transaction<'_>, ord: usize, ws: &WorkspaceId, s: &Snapshot) -> rusqlite::Result<()> {
    let id = ws.as_str();
    for table in ["sections", "conversations", "saved"] {
        tx.execute(&format!("DELETE FROM {table} WHERE workspace = ?1"), [id])?;
    }
    tx.execute("DELETE FROM workspaces WHERE id = ?1", [id])?;
    tx.execute("UPDATE users SET ord = NULL WHERE workspace = ?1", [id])?;
    let s = s.only(ws);
    for w in &s.workspaces {
        tx.execute(
            "INSERT INTO workspaces (id, ord, name, color, me) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![w.id.as_str(), int(ord as u64)?, w.name.unsanitized(), w.color.0, w.me.as_str()],
        )?;
    }
    for (i, u) in s.users.iter().enumerate() {
        put_user(tx, u, Some(i))?;
    }
    for (i, sec) in s.sections.iter().enumerate() {
        tx.execute(
            "INSERT INTO sections (workspace, ord, id, name, kind) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, int(i as u64)?, sec.id.as_str(), sec.name.unsanitized(), section_code(sec.kind)],
        )?;
    }
    for (i, c) in s.conversations.iter().enumerate() {
        put_conversation(tx, i, c)?;
    }
    for (i, item) in s.saved.iter().flatten().enumerate() {
        put_saved(tx, i, item)?;
    }
    let known = if s.saved.is_some() { "known" } else { "unknown" };
    for (k, v) in [("snapshot.taken:", s.taken.0.to_string()), ("snapshot.saved:", known.to_string())] {
        tx.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = ?2",
            [&format!("{k}{id}"), &v],
        )?;
    }
    Ok(())
}

fn all<T>(
    conn: &Connection,
    sql: &str,
    ws: &str,
    f: impl Fn(&Row<'_>) -> rusqlite::Result<T>,
) -> rusqlite::Result<Vec<T>> {
    let mut st = conn.prepare(sql)?;
    st.query_map([ws], |r| f(r))?.collect()
}

/// Workspace `ws`'s part, when the cache has one.
fn read(conn: &Connection, ws: &WorkspaceId) -> rusqlite::Result<Option<Snapshot>> {
    use rusqlite::OptionalExtension;
    let id = ws.as_str();
    let meta = |k: &str| {
        conn.query_row("SELECT value FROM meta WHERE key = ?1", [format!("{k}{id}")], |r| r.get::<_, String>(0))
            .optional()
    };
    let (Some(taken), Some(known)) = (meta("snapshot.taken:")?, meta("snapshot.saved:")?) else { return Ok(None) };
    let taken = Ts(taken.parse().map_err(|_| damaged("snapshot time"))?);
    let workspaces = all(conn, "SELECT * FROM workspaces WHERE id = ?1", id, |r| {
        Ok(Workspace {
            id: WorkspaceId::new(r.get::<_, String>("id")?),
            name: Remote::new(r.get::<_, String>("name")?),
            color: WorkspaceColor(r.get("color")?),
            me: UserId::new(r.get::<_, String>("me")?),
        })
    })?;
    if workspaces.is_empty() {
        return Ok(None);
    }
    let users = all(conn, "SELECT * FROM users WHERE workspace = ?1 AND ord IS NOT NULL ORDER BY ord", id, user)?;
    let sections = all(conn, "SELECT * FROM sections WHERE workspace = ?1 ORDER BY ord", id, |r| {
        Ok(Section {
            id: SectionId::new(r.get::<_, String>("id")?),
            workspace: WorkspaceId::new(r.get::<_, String>("workspace")?),
            name: Remote::new(r.get::<_, String>("name")?),
            kind: section_kind(r.get("kind")?)?,
        })
    })?;
    let conversations = all(conn, "SELECT * FROM conversations WHERE workspace = ?1 ORDER BY ord", id, conversation)?;
    let saved = match known.as_str() {
        "known" => Some(all(conn, "SELECT * FROM saved WHERE workspace = ?1 ORDER BY ord", id, saved)?),
        "unknown" => None,
        _ => return Err(damaged("saved marker")),
    };
    Ok(Some(Snapshot { workspaces, users, sections, conversations, saved, taken }))
}

impl Store {
    /// Keep each workspace's part of `snapshot` as its last boot's, in place of the one before;
    /// a workspace `snapshot` does not list keeps its own (the writer only).
    pub fn put_snapshot(&mut self, snapshot: &Snapshot) -> rusqlite::Result<()> {
        if self.read_only() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        for (ord, w) in snapshot.workspaces.iter().enumerate() {
            write(&tx, ord, &w.id, snapshot)?;
        }
        tx.commit()
    }

    /// The last boots' snapshot of the workspaces `expected`, in that order, when the cache has a
    /// part of each one; `None` otherwise.
    pub fn snapshot(&self, expected: &[WorkspaceId]) -> Result<Option<Snapshot>, rusqlite::Error> {
        // One read transaction: what another process commits meanwhile is not half seen.
        let tx = self.conn.unchecked_transaction()?;
        let mut whole = Snapshot { saved: Some(Vec::new()), ..Snapshot::default() };
        let mut unknown = false;
        for ws in expected {
            let Some(s) = read(&tx, ws)? else {
                tx.commit()?;
                return Ok(None);
            };
            whole.workspaces.extend(s.workspaces);
            whole.users.extend(s.users);
            whole.sections.extend(s.sections);
            whole.conversations.extend(s.conversations);
            match s.saved {
                Some(items) => whole.saved.get_or_insert_with(Vec::new).extend(items),
                None => unknown = true,
            }
            whole.taken = whole.taken.max(s.taken);
        }
        tx.commit()?;
        if unknown {
            whole.saved = None;
        }
        Ok((!expected.is_empty()).then_some(whole))
    }
}

#[cfg(test)]
mod tests;
