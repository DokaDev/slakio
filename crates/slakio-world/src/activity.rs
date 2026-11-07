//! The demo's Activity: the user's notices in both workspaces, each about a message of the
//! world — mentions of the user, of everyone, of a group, of a word; new replies in a long
//! thread; a reaction; a DM; one in the hostile channel — and, noticed before those, a mention
//! every [`BIG_EVERY`] messages of `big-history`, so Activity has many pages. Newest first, as
//! Slack lists them.

use crate::{World, names};
use slakio_core::model::{ConversationId, Message, Ts, UserId, WorkspaceId};

/// A mention every this many messages of `big-history`.
pub const BIG_EVERY: usize = 40;

const HOUR: u64 = 3_600_000_000;

/// A notice of the world, with the kind Slack names it by (`at_user`, `thread_v2`, …).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Noticed {
    pub key: String,
    pub kind: &'static str,
    /// When it happened.
    pub at: Ts,
    pub conversation: ConversationId,
    /// The message it is about (the newest reply of a thread).
    pub message: Message,
    pub thread: Option<Ts>,
    /// Who reacted (a reaction), and with what.
    pub reactor: Option<(UserId, &'static str)>,
    pub unread: bool,
}

/// (workspace, conversation, how many messages back from its newest, kind, unread).
type Spec = (&'static str, &'static str, usize, &'static str, bool);

const ITEMS: &[Spec] = &[
    ("TDEMOA", "incidents", 0, "at_user", true),
    ("TDEMOA", "backend", 1, "at_channel", true),
    ("TDEMOA", names::HOSTILE, 0, "at_user", true),
    ("TDEMOA", "frontend", 2, "at_user_group", false),
    ("TDEMOA", "general", 2, "keyword", false),
    ("TDEMOA", "alerts", 0, "message_reaction", false),
    ("TDEMOA", "Minsu Kim", 0, "dm", true),
    ("TDEMOA", names::LONG_THREADS, 0, "thread_v2", true),
    ("TDEMOA", "deploys", 3, "at_user", false),
    ("TDEMOB", "side-project", 0, "at_user", true),
    ("TDEMOB", "ideas", 0, "thread_v2", false),
    ("TDEMOB", "general", 0, "at_channel", false),
];

impl World {
    /// Every notice of `workspace`, newest first.
    pub fn activity(&self, workspace: &WorkspaceId) -> Vec<Noticed> {
        let mut out: Vec<Noticed> = ITEMS
            .iter()
            .enumerate()
            .filter(|(_, s)| s.0 == workspace.as_str())
            .filter_map(|(i, s)| self.noticed(i, s))
            .collect();
        if let Some(c) = self.find(workspace.as_str(), names::BIG_HISTORY) {
            let id = c.id.clone();
            // Listed as noticed before every notice above (an hour apart, newest first), so the
            // first page shows each kind.
            let before = out.iter().map(|n| n.at.0).min().unwrap_or(u64::MAX);
            let n = self.message_count(&id);
            for (k, i) in (7..n).step_by(BIG_EVERY).enumerate() {
                let Some(m) = self.message(&id, n - 1 - i) else { continue };
                out.push(Noticed {
                    key: format!("big-{i}"),
                    kind: "at_user",
                    at: Ts(before.saturating_sub((k as u64 + 1) * HOUR)),
                    conversation: id.clone(),
                    message: m,
                    thread: None,
                    reactor: None,
                    unread: i % 3 == 0,
                });
            }
        }
        out.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.key.cmp(&b.key)));
        out
    }

    fn noticed(&self, i: usize, &(ws, name, back, kind, unread): &Spec) -> Option<Noticed> {
        let c = self.find(ws, name)?;
        let n = self.message_count(&c.id);
        let key = format!("n{i:03}");
        let base = |message: Message, thread: Option<Ts>| Noticed {
            key: key.clone(),
            kind,
            at: message.ts,
            conversation: c.id.clone(),
            message,
            thread,
            reactor: None,
            unread,
        };
        if kind == "thread_v2" {
            // The newest message with replies (the long thread's is its first), at its last reply.
            let parent = (0..n).rev().find(|&p| self.message(&c.id, p).is_some_and(|m| m.thread.is_some()))?;
            let root = self.message(&c.id, parent)?;
            let reply = self.reply(&c.id, parent, root.thread?.replies - 1)?;
            return Some(base(reply, Some(root.ts)));
        }
        let message = self.message(&c.id, n.checked_sub(1 + back)?)?;
        let mut out = base(message, None);
        if kind == "message_reaction" {
            let me = self.me(&c.workspace);
            let s = self.snapshot();
            let who = s.users.iter().find(|u| u.workspace == c.workspace && Some(&u.id) != me && !u.bot)?;
            out.reactor = Some((who.id.clone(), "eyes"));
            out.at = Ts(out.at.0 + 60_000_000);
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests;
