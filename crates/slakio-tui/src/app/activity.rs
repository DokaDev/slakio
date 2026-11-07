//! Activity (`Space a`): the user's notices of every workspace signed in to, newest first —
//! mentions (of them, of everyone, of a group of theirs, of a word), new replies in threads they
//! follow, reactions to their messages, DMs — as Slack lists them, read or not. `Enter` opens the
//! message as a Slack link to it opens ([`super::jump`]): the conversation at the message, a
//! reply's thread beside it; `l` opens it staying in the list.
//!
//! It is read when it shows, workspace by workspace (its newest page, unless read less than
//! [`FRESH`] ago), and a page further on each time the cursor nears its end. A mention that comes
//! live is listed at once; the newest page is read again for it once things settle
//! ([`SETTLE`]), and never more often than every [`FRESH`]: so too after the live updates came
//! back. Nothing is read for Activity while it does not show; a page that failed is asked again
//! by `r`, never by itself.

use super::App;
use super::model::Row;
use super::query::Focus;
use super::shell::View;
use crate::permalink::{Place, SlackLink};
use slakio_core::backend::{Command, FailKind, Feed, Generation, Notice};
use slakio_core::i18n::Msg;
use slakio_core::model::{ConversationId, Message, Target, Ts, WorkspaceId};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A workspace's newest page read this long ago is not read again when Activity shows.
pub const FRESH: Duration = Duration::from_secs(30);

/// How long after a live mention (or the live updates coming back) the newest page is read again.
pub const SETTLE: Duration = Duration::from_secs(2);

/// Rows before the end of the list at which the next page is asked for.
pub const NEAR_END: usize = 6;

/// What the app asked of each workspace's Activity, and when.
#[derive(Clone, Debug, Default)]
pub(crate) struct Activity {
    feeds: HashMap<WorkspaceId, Part>,
    /// Activity showed at the last look: it is read again only when it shows anew.
    shown: bool,
}

/// One workspace's part of Activity.
#[derive(Clone, Debug, Default)]
struct Part {
    /// The page on its way: its request, and whether it is the newest.
    asked: Option<(Generation, bool)>,
    /// Where its next page starts, when there is one.
    next: Option<String>,
    /// When its newest page was read last.
    read: Option<Instant>,
    /// Why the last page asked for failed, and whether it was the newest.
    failed: Option<(FailKind, bool)>,
    /// When its newest page is to be read again (a live mention, the live updates back).
    again: Option<Instant>,
}

/// What the end of Activity's list says, if anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Foot {
    /// A page is on its way.
    Loading,
    /// A page failed (`r` asks again when that may help).
    Failed(FailKind),
}

impl App {
    /// The notice under the list's cursor, in Activity.
    pub fn notice_at_cursor(&self) -> Option<&(WorkspaceId, Notice)> {
        match self.shell.rows(&self.model).get(self.shell.list_cursor) {
            Some(Row::Notice(i)) if self.shell.view == View::Activity => self.model.notices.get(*i),
            _ => None,
        }
    }

    /// Nothing of Activity is loaded yet, and why: on its way (`None`), or failed.
    pub fn activity_unloaded(&self) -> Option<Option<FailKind>> {
        if !self.model.notices.is_empty() || self.activity.feeds.values().any(|f| f.read.is_some()) {
            return None;
        }
        let failed = self.activity.feeds.values().find_map(|f| f.failed.as_ref().map(|(k, _)| k.clone()));
        let waiting = self.activity.feeds.values().any(|f| f.asked.is_some());
        match failed {
            Some(k) if !waiting => Some(Some(k)),
            _ => Some(None),
        }
    }

    /// What the end of Activity's list says.
    pub fn activity_foot(&self) -> Option<Foot> {
        let feeds = self.activity.feeds.values();
        if feeds.clone().any(|f| f.asked.is_some()) {
            return Some(Foot::Loading);
        }
        feeds.filter_map(|f| f.failed.as_ref()).map(|(k, _)| Foot::Failed(k.clone())).next()
    }

    /// The workspaces Activity reads: those whose boot answered in this run, but one Slack
    /// signed out.
    fn activity_workspaces(&self) -> Vec<WorkspaceId> {
        (self.model.workspaces().iter().map(|w| w.id.clone()))
            .filter(|w| self.model.is_fresh(w) && !self.signed_out_in(w))
            .collect()
    }

    /// Ask for a page of `ws`, the newest without `cursor`: not while the newest is on its way,
    /// nor a later page while one is (the newest goes all the same).
    fn ask_activity(&mut self, ws: &WorkspaceId, cursor: Option<String>) {
        let first = cursor.is_none();
        let feed = self.activity.feeds.entry(ws.clone()).or_default();
        if feed.asked.is_some_and(|(_, f)| f || !first) {
            return;
        }
        let id = self.work.requests.ask(Some(ws.clone()), Command::Activity { cursor });
        (feed.asked, feed.failed) = (Some((id, first)), None);
        if first {
            feed.again = None;
        }
    }

    /// Look at whether Activity shows, at `now`: shown anew, each workspace's newest page is read
    /// unless it was a moment ago; after a key (`keyed`), the next pages when the cursor is near
    /// the end of the list (an answer alone never asks for more but through [`App::on_activity`]).
    pub(super) fn watch_activity(&mut self, now: Instant, keyed: bool) {
        let shows = self.shell.view == View::Activity && !self.shell.list_hidden && self.backend.is_some();
        if shows {
            let anew = !self.activity.shown;
            for ws in self.activity_workspaces() {
                // A workspace booted while it shows is read then.
                let ask = match self.activity.feeds.get(&ws) {
                    None => true,
                    Some(f) => anew && f.asked.is_none() && f.read.is_none_or(|at| now >= at + FRESH),
                };
                if ask {
                    self.ask_activity(&ws, None);
                }
            }
        }
        self.activity.shown = shows;
        if shows && keyed {
            self.activity_more();
        }
    }

    /// The cursor nears the end of Activity's list: the next page of each workspace that has
    /// one (and has nothing on its way, nor a failure waiting for `r`).
    fn activity_more(&mut self) {
        let rows = self.model.notice_rows().len();
        if self.shell.list_cursor + NEAR_END < rows {
            return;
        }
        let more: Vec<(WorkspaceId, String)> = (self.activity.feeds.iter())
            .filter(|(_, f)| f.asked.is_none() && f.failed.is_none())
            .filter_map(|(ws, f)| Some((ws.clone(), f.next.clone()?)))
            .collect();
        for (ws, next) in more {
            self.ask_activity(&ws, Some(next));
        }
    }

    /// A page of Activity came for request `id`: listed when it is the one asked for, the cursor
    /// kept on its notice.
    pub(super) fn on_activity(&mut self, id: Generation, feed: Feed, now: Instant) -> bool {
        let Some((ws, first)) = (self.activity.feeds.iter())
            .find_map(|(ws, f)| f.asked.filter(|(a, _)| *a == id).map(|(_, f)| (ws.clone(), f)))
        else {
            return false;
        };
        let anchor = self.shell.anchor(&self.model);
        // A page of nothing Activity shows asks for no more by itself (only a key near the end
        // does): a run of such pages is never read through unasked.
        let added = !feed.notices.is_empty();
        self.model.add_users(&ws, &feed.users);
        self.model.notices.put(&ws, first, feed.notices);
        let f = self.activity.feeds.entry(ws).or_default();
        (f.asked, f.failed, f.next) = (None, None, feed.next);
        if first {
            f.read = Some(now);
        }
        let h = self.list_height();
        self.shell.clamp(&self.model, h);
        self.shell.restore(anchor, &self.model, h);
        if self.activity.shown && added {
            self.activity_more();
        }
        true
    }

    /// Request `id` failed: when it is a page of Activity asked for, its end says why; its
    /// workspace is returned.
    pub(super) fn activity_failed(&mut self, id: Generation, kind: &FailKind) -> Option<WorkspaceId> {
        let (ws, f) = self.activity.feeds.iter_mut().find(|(_, f)| f.asked.is_some_and(|(a, _)| a == id))?;
        let first = f.asked.take().is_some_and(|(_, first)| first);
        f.failed = Some((kind.clone(), first));
        Some(ws.clone())
    }

    /// `r` in Activity: the pages that failed for a reason that may pass are asked for again.
    /// `false` when none did.
    pub(super) fn retry_activity(&mut self) -> bool {
        let again: Vec<(WorkspaceId, Option<String>)> = (self.activity.feeds.iter())
            .filter(|(_, f)| f.asked.is_none() && f.failed.as_ref().is_some_and(|(k, _)| k.retryable()))
            .map(|(ws, f)| {
                (ws.clone(), if f.failed.as_ref().is_some_and(|(_, first)| *first) { None } else { f.next.clone() })
            })
            .collect();
        for (ws, cursor) in &again {
            self.ask_activity(ws, cursor.clone());
        }
        !again.is_empty()
    }

    /// A message that mentions the user came live in `conversation` of `ws` (a reply under
    /// `thread`): listed at once when Activity was read, and its newest page read again once
    /// things settle (not sooner than [`FRESH`] after it was read last).
    pub(super) fn live_mention(
        &mut self,
        ws: &WorkspaceId,
        conversation: &ConversationId,
        thread: Option<Ts>,
        m: &Message,
        now: Instant,
    ) -> bool {
        if self.activity.feeds.get(ws).is_none_or(|f| f.read.is_none()) {
            return false;
        }
        let listed = self.model.notices.live(ws, conversation, thread, m);
        self.read_again(ws, now);
        listed
    }

    /// Read the newest page of `ws` again soon (once read before): the live updates are back.
    pub(super) fn activity_back(&mut self, ws: &WorkspaceId, now: Instant) {
        if self.activity.feeds.get(ws).is_some_and(|f| f.read.is_some()) {
            self.read_again(ws, now);
        }
    }

    fn read_again(&mut self, ws: &WorkspaceId, now: Instant) {
        let f = self.activity.feeds.entry(ws.clone()).or_default();
        let at = (now + SETTLE).max(f.read.map_or(now, |r| r + FRESH));
        f.again = Some(f.again.map_or(at, |a| a.min(at)));
    }

    /// When a workspace's newest page is to be read again.
    pub(super) fn activity_deadline(&self) -> Option<Instant> {
        self.activity.feeds.values().filter_map(|f| f.again).min()
    }

    /// At `now`: the newest pages due are read again while Activity shows; while it does not,
    /// they are read when it shows next.
    pub(super) fn activity_tick(&mut self, now: Instant) -> bool {
        let due: Vec<WorkspaceId> = (self.activity.feeds.iter())
            .filter(|(_, f)| f.again.is_some_and(|a| a <= now))
            .map(|(w, _)| w.clone())
            .collect();
        for ws in &due {
            if self.activity.shown {
                self.ask_activity(ws, None);
            } else if let Some(f) = self.activity.feeds.get_mut(ws) {
                (f.again, f.read) = (None, None);
            }
        }
        !due.is_empty()
    }

    /// Forget what was asked of Activity in `ws` (signed in again as someone else).
    pub(super) fn forget_activity(&mut self, ws: &WorkspaceId) {
        self.activity.feeds.remove(ws);
    }

    /// The notice under the cursor opened where its message is, as a Slack link to it opens;
    /// `focus`: the pane opened gets the keyboard, else the list keeps it. A conversation the
    /// workspace does not list says so.
    pub(super) fn open_notice(&mut self, focus: bool, now: Instant) {
        let Some((ws, n)) = self.notice_at_cursor().cloned() else { return };
        let listed = self.model.conversations().iter().any(|c| c.workspace == ws && c.id == n.conversation);
        if !listed {
            self.info(Msg::JumpUnlisted { host: self.workspace_label(&ws) }, now);
            return;
        }
        let target = Target::Conversation { workspace: ws.clone(), conversation: n.conversation.clone() };
        let link = SlackLink {
            place: Place::Team(ws.as_str().to_string()),
            conversation: n.conversation.as_str().to_string(),
            message: Some(n.ts),
            thread: n.thread,
        };
        self.jump_to(target, &link, "", None, now);
        // No web link to offer should Slack refuse to read it.
        self.jumps.asked = None;
        if !focus {
            self.set_focus(Focus::List);
        }
    }
}
