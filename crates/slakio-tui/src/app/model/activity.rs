//! Activity as the backends listed it: the notices of every workspace loaded so far, merged
//! newest first (each workspace's in Slack's order, two workspaces' by when they happened). A
//! workspace's first page read again says what Slack holds from its oldest notice on, and the
//! older pages read before stay; a later page adds what it does not hold yet. A mention that
//! came live is listed at once, until a first page holds it or [`LIVE_KEEP`] has passed.

use super::{Model, Row};
use slakio_core::backend::{Notice, NoticeKind};
use slakio_core::model::{ConversationId, Message, Ts, WorkspaceId};
use std::time::Duration;

/// The most notices kept across workspaces: the oldest go first. They are the oldest of every
/// workspace, at the list's end, and no page is asked once it is reached, so what is listed
/// stays whole from the newest on.
pub const NOTICES_MAX: usize = 2_000;

/// The key of a notice listed from a live mention (never one of Slack's).
const LIVE: &str = "live:";

/// How long a mention listed live stays while no first page holds it (Slack may list it late).
pub const LIVE_KEEP: Duration = Duration::from_secs(10 * 60);

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

    /// A page of workspace `ws`, at `now`. The first (newest) replaces what `ws` lists from its
    /// oldest notice on, and the notices it holds again; what is older stays. A mention listed
    /// live stays until the page holds its message, or for [`LIVE_KEEP`]. A later page adds the
    /// notices not held yet. How many of the page's notices are listed that were not before
    /// (once the oldest beyond [`NOTICES_MAX`] went).
    pub fn put(&mut self, ws: &WorkspaceId, first: bool, notices: Vec<Notice>, now: Ts) -> usize {
        if first {
            let oldest = notices.last().map(|n| n.at);
            let keep = u64::try_from(LIVE_KEEP.as_micros()).unwrap_or(u64::MAX);
            self.all.retain(|(w, n)| {
                if w != ws {
                    return true;
                }
                if n.key.starts_with(LIVE) {
                    let held = notices.iter().any(|p| p.conversation == n.conversation && p.ts == n.ts);
                    return !held && now.0.saturating_sub(n.at.0) < keep;
                }
                oldest.is_some_and(|o| n.at < o) && !notices.iter().any(|p| p.key == n.key)
            });
        }
        let mut new = Vec::new();
        for n in notices {
            if !self.holds(ws, &n.key) {
                new.push(n.key.clone());
                self.all.push((ws.clone(), n));
            }
        }
        self.order();
        new.iter().filter(|k| self.holds(ws, k)).count()
    }

    /// `ws` lists the notice of `key`.
    pub fn holds(&self, ws: &WorkspaceId, key: &str) -> bool {
        self.all.iter().any(|(w, n)| w == ws && n.key == key)
    }

    /// Forget the notices of `ws` Slack listed, keeping those listed live.
    pub fn forget_listed(&mut self, ws: &WorkspaceId) {
        self.all.retain(|(w, n)| w != ws || n.key.starts_with(LIVE));
    }

    /// A mention of the user that came live in `conversation` (a reply under `thread`): listed
    /// unread unless Activity holds that message already. `true` when it was listed.
    pub fn live(&mut self, ws: &WorkspaceId, conversation: &ConversationId, thread: Option<Ts>, m: &Message) -> bool {
        if self.all.iter().any(|(w, n)| w == ws && n.conversation == *conversation && n.ts == m.ts) {
            return false;
        }
        // Every field came from the live updates, which checked their shape.
        debug_assert!(!conversation.as_str().is_empty() && m.ts.0 > 0 && thread.is_none_or(|t| t.0 > 0));
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
