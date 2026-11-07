//! Activity as the backends listed it: the notices of every workspace loaded so far, merged
//! newest first (each workspace's in Slack's order, two workspaces' by when they happened). A
//! workspace's first page replaces what it listed before; a later page adds what it does not
//! hold yet. A mention that came live is listed at once, until the next first page says what
//! Slack holds.

use super::{Model, Row};
use slakio_core::backend::{Notice, NoticeKind};
use slakio_core::model::{ConversationId, Message, Ts, WorkspaceId};

/// The most notices kept across workspaces: the oldest go first.
pub const NOTICES_MAX: usize = 2_000;

/// The key of a notice listed from a live mention (never one of Slack's).
const LIVE: &str = "live:";

/// The notices of Activity, newest first.
#[derive(Clone, Debug, Default)]
pub struct Notices {
    all: Vec<(WorkspaceId, Notice)>,
}

impl Notices {
    pub fn get(&self, i: usize) -> Option<&(WorkspaceId, Notice)> {
        self.all.get(i)
    }

    pub fn len(&self) -> usize {
        self.all.len()
    }

    pub fn is_empty(&self) -> bool {
        self.all.is_empty()
    }

    /// A page of workspace `ws`: the first replaces what it listed (live ones too), a later one
    /// adds the notices it does not hold yet.
    pub fn put(&mut self, ws: &WorkspaceId, first: bool, notices: Vec<Notice>) {
        if first {
            self.all.retain(|(w, _)| w != ws);
        }
        for n in notices {
            if !self.all.iter().any(|(w, k)| w == ws && k.key == n.key) {
                self.all.push((ws.clone(), n));
            }
        }
        self.order();
    }

    /// A mention of the user that came live in `conversation` (a reply under `thread`): listed
    /// unread unless Activity holds that message already. `true` when it was listed.
    pub fn live(&mut self, ws: &WorkspaceId, conversation: &ConversationId, thread: Option<Ts>, m: &Message) -> bool {
        if self.all.iter().any(|(w, n)| w == ws && n.conversation == *conversation && n.ts == m.ts) {
            return false;
        }
        let notice = Notice {
            key: format!("{LIVE}{conversation}:{}", m.ts),
            kind: NoticeKind::Mention,
            at: m.ts,
            conversation: conversation.clone(),
            ts: m.ts,
            thread,
            user: m.user.clone(),
            text: Some(m.text.clone()),
            reaction: None,
            unread: true,
        };
        self.all.push((ws.clone(), notice));
        self.order();
        true
    }

    /// Forget every notice of `ws`.
    pub fn forget(&mut self, ws: &WorkspaceId) {
        self.all.retain(|(w, _)| w != ws);
    }

    /// Keep only the workspaces `keep` lets through.
    pub fn retain(&mut self, keep: impl Fn(&WorkspaceId) -> bool) {
        self.all.retain(|(w, _)| keep(w));
    }

    fn order(&mut self) {
        self.all.sort_by(|(wa, a), (wb, b)| b.at.cmp(&a.at).then_with(|| (wa, &a.key).cmp(&(wb, &b.key))));
        self.all.truncate(NOTICES_MAX);
    }
}

impl Model {
    /// Activity's rows: two a notice (what, where and when; then who and the text).
    pub fn notice_rows(&self) -> Vec<Row> {
        (0..self.notices.len()).flat_map(|i| [Row::Notice(i), Row::NoticeMeta(i)]).collect()
    }
}

#[cfg(test)]
mod tests;
