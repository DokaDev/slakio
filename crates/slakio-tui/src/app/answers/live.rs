//! What the backend pushes as it happens ([`Event::Live`](slakio_core::backend::Event::Live)):
//! messages into the panes that show them, the list's counts, and whether a workspace's live
//! updates arrive — said briefly in the status line each time that changes (with several
//! workspaces, whose). When they come back after they were lost, the open conversations and
//! threads are read again, since nothing missed meanwhile is replayed, and the list takes Slack's
//! counts again.

use super::super::App;
use super::super::status::Level;
use super::super::work::live::Place;
use slakio_core::backend::{Connection, Live};
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::{User, Workspace, WorkspaceId};
use std::time::Instant;

impl App {
    /// `live` arrived at `now`. `true` when the screen changed. The list's cursor stays on its
    /// row (a conversation, a header) when rows come and go around it, a header that is gone
    /// among them. Sections of the workspace shown that come before the user moved it from where
    /// the boot put it put it at the top again.
    pub(super) fn on_live(&mut self, live: Live, now: Instant) -> bool {
        let anchor = self.shell.anchor(&self.model);
        let shown = self.shell.current_id(&self.model);
        let rehome = matches!(&live, Live::Sections { workspace, .. } if Some(workspace) == shown.as_ref())
            && self.shell.untouched();
        let changed = self.live_update(live, now);
        if changed {
            let h = self.list_height();
            self.shell.clamp(&self.model, h);
            if rehome {
                self.shell.rehome(&self.model, h);
            } else {
                self.shell.restore(anchor, &self.model, h);
            }
        }
        changed
    }

    /// Take `live` in; `true` when the screen changed.
    fn live_update(&mut self, live: Live, now: Instant) -> bool {
        match live {
            Live::Message { workspace, conversation, thread, broadcast, changed, mention, message, users } => {
                self.model.add_users(&workspace, &users);
                let at = Place { workspace: &workspace, conversation: &conversation, thread, broadcast };
                let mut drawn = self.work.on_live_message(at, changed, &message, &self.model);
                // A reply counts in the conversation only when shown there too; the user's own
                // messages (sent from another client) are read, yet make it the newest.
                let counts = !changed && (thread.is_none() || broadcast);
                if counts && self.model.me(&workspace) != Some(&message.user) {
                    drawn |= self.model.bump(&workspace, &conversation, mention, message.ts);
                } else if counts {
                    drawn |= self.model.touch(&workspace, &conversation, message.ts);
                }
                // A new mention is listed in Activity (a reply too), and Activity read again.
                if mention && !changed && self.model.me(&workspace) != Some(&message.user) {
                    drawn |= self.live_mention(&workspace, &conversation, thread, &message, now);
                }
                drawn
            }
            Live::Deleted { workspace, conversation, thread, ts } => {
                let at = Place { workspace: &workspace, conversation: &conversation, thread, broadcast: false };
                self.work.on_live_deleted(at, ts)
            }
            Live::Marked { workspace, conversation, unread, mentions } => {
                self.model.mark(&workspace, &conversation, unread, mentions)
            }
            Live::Counts { workspace, counts } => self.model.recount(&workspace, &counts),
            // Only people shown already: a workspace's every profile change comes this way. It
            // tells their names and photo; whether they are around, a bot or of another
            // organization stays as known (a change does not say it). A DM, a group DM, is renamed with them.
            Live::People { workspace, users } => {
                let known: Vec<_> = (users.into_iter())
                    .filter_map(|u| {
                        let old = self.model.user(&workspace, &u.id)?;
                        let User { id, workspace: ws, name, display_name, avatar, no_photo, avatar_guessed, .. } = u;
                        Some(User {
                            id,
                            workspace: ws,
                            name,
                            display_name,
                            avatar,
                            no_photo,
                            avatar_guessed,
                            ..old.clone()
                        })
                    })
                    .collect();
                self.model.rename_dms(&workspace, &known);
                self.model.add_users(&workspace, &known);
                !known.is_empty()
            }
            Live::Connection { workspace, state } => self.on_connection(workspace, state, now),
            Live::Sections { workspace, sections, places } => self.model.resection(&workspace, sections, &places),
        }
    }

    /// Whether some workspace's live updates stopped (the status line's steady mark): those
    /// workspaces when several are signed in (their stripes show), and whether the request
    /// budget holds any of them. `None` while every one heard of arrives.
    pub fn live_off(&self) -> Option<(Vec<&Workspace>, bool)> {
        // A workspace Slack signed out has a mark of its own.
        let off: Vec<(&Workspace, bool)> = (self.model.workspaces().iter())
            .filter(|w| !self.signed_out_in(&w.id))
            .filter_map(|w| Some((w, self.model.live_off(&w.id)?)))
            .collect();
        if off.is_empty() {
            return None;
        }
        let held = off.iter().any(|(_, held)| *held);
        let whose = if self.boots.len() > 1 { off.into_iter().map(|(w, _)| w).collect() } else { Vec::new() };
        Some((whose, held))
    }

    /// The live updates of `ws` started or stopped. While they do not arrive, the status line
    /// keeps a mark of it besides the notices.
    fn on_connection(&mut self, ws: WorkspaceId, state: Connection, now: Instant) -> bool {
        let ever = self.model.live.get(&ws).is_some_and(|(_, ever)| *ever);
        let (label, level) = match state {
            Connection::Connected { catch_up } => {
                if catch_up {
                    self.work.catch_up(&ws, false);
                    self.activity_back(&ws, now);
                }
                (if ever { Label::LiveBack } else { Label::LiveOn }, Level::Info)
            }
            Connection::Lost => (if ever { Label::LiveLost } else { Label::LiveWaiting }, Level::Warning),
            Connection::Held => (Label::LiveHeld, Level::Warning),
            // Said as a sign-out, not as live updates that stopped.
            Connection::SignedOut => {
                self.model.live.insert(ws.clone(), (state, ever));
                self.signed_out(ws, now);
                return true;
            }
        };
        let on = matches!(state, Connection::Connected { .. });
        self.model.live.insert(ws.clone(), (state, ever || on));
        let whose = self.model.workspace(&ws).filter(|_| self.boots.len() > 1).cloned();
        let name = whose.as_ref().map(|w| w.name.line().into_string());
        let msg = match &name {
            Some(n) => Msg::FailWorkspace { workspace: n.clone(), why: self.i18n.label(label).to_string() },
            None => Msg::Label(label),
        };
        let about = whose.map(|w| w.id);
        // What was said of this before is no longer true: said in its place. Lost is said at
        // once (and again after a notice that takes its place); on and back wait their turn
        // behind anything else showing. Back after a loss that was not seen yet says nothing:
        // the loss is not said late either, and the status line's mark covered it.
        let words: Vec<String> = WORDS.iter().map(|l| self.i18n.label(*l).to_string()).collect();
        let stale = |m: &Msg| match m {
            Msg::Label(l) => WORDS.contains(l),
            Msg::FailWorkspace { workspace, why } => name.as_ref() == Some(workspace) && words.contains(why),
            _ => false,
        };
        if label == Label::LiveBack && self.status.unseen(now, stale) {
            self.status.forget(now, stale);
        } else {
            self.status.restate_about(msg, level, about, now, stale);
        }
        true
    }

    /// Forget what was said of the live updates of `ws` (Slack signed it out: none come again).
    pub(in crate::app) fn forget_live(&mut self, ws: &WorkspaceId, now: Instant) {
        let name = self.model.workspace(ws).map(|w| w.name.line().into_string());
        let words: Vec<String> = WORDS.iter().map(|l| self.i18n.label(*l).to_string()).collect();
        let alone = self.boots.len() <= 1;
        self.status.forget(now, |m| match m {
            Msg::Label(l) => alone && WORDS.contains(l),
            Msg::FailWorkspace { workspace, why } => name.as_ref() == Some(workspace) && words.contains(why),
            _ => false,
        });
    }
}

/// The status line's words for live updates.
const WORDS: [Label; 5] = [Label::LiveOn, Label::LiveBack, Label::LiveLost, Label::LiveWaiting, Label::LiveHeld];
