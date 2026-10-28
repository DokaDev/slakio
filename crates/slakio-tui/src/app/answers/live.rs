//! What the backend pushes as it happens ([`Event::Live`](slakio_core::backend::Event::Live)):
//! messages into the panes that show them, the list's counts, and whether a workspace's live
//! updates arrive — said briefly in the status line each time that changes (with several
//! workspaces, whose). When they come back after they were lost, the open conversations are read
//! again, since nothing missed meanwhile is replayed.

use super::super::App;
use super::super::status::Level;
use super::super::work::live::Place;
use slakio_core::backend::{Connection, Live};
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::WorkspaceId;
use std::time::Instant;

impl App {
    /// `live` arrived at `now`. `true` when the screen changed.
    pub(super) fn on_live(&mut self, live: Live, now: Instant) -> bool {
        match live {
            Live::Message { workspace, conversation, thread, broadcast, changed, mention, message, users } => {
                self.model.add_users(&workspace, &users);
                let at = Place { workspace: &workspace, conversation: &conversation, thread, broadcast };
                let mut drawn = self.work.on_live_message(at, changed, &message, &self.model);
                // A reply counts in the conversation only when shown there too; the user's own
                // messages (sent from another client) are read.
                let counts = !changed && (thread.is_none() || broadcast);
                if counts && self.model.me(&workspace) != Some(&message.user) {
                    drawn |= self.model.bump(&workspace, &conversation, mention);
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
            Live::Connection { workspace, state } => self.on_connection(workspace, state, now),
        }
    }

    /// The live updates of `ws` started or stopped.
    fn on_connection(&mut self, ws: WorkspaceId, state: Connection, now: Instant) -> bool {
        let (_, ever) = self.model.live.get(&ws).copied().unwrap_or_default();
        let (label, level) = match state {
            Connection::Connected { catch_up } => {
                if catch_up {
                    self.work.catch_up(&ws);
                }
                self.model.live.insert(ws.clone(), (true, true));
                (if ever { Label::LiveBack } else { Label::LiveOn }, Level::Info)
            }
            Connection::Lost => {
                self.model.live.insert(ws.clone(), (false, ever));
                (if ever { Label::LiveLost } else { Label::LiveWaiting }, Level::Warning)
            }
        };
        let whose = self.model.workspace(&ws).filter(|_| self.boots.len() > 1).cloned();
        let name = whose.as_ref().map(|w| w.name.line().into_string());
        let msg = match &name {
            Some(n) => Msg::FailWorkspace { workspace: n.clone(), why: self.i18n.label(label).to_string() },
            None => Msg::Label(label),
        };
        let about = whose.map(|w| w.id);
        // What was said of this before is no longer true: said in its place. Lost is said at
        // once; on and back wait their turn behind anything else showing.
        let words: Vec<String> = WORDS.iter().map(|l| self.i18n.label(*l).to_string()).collect();
        let stale = |m: &Msg| match m {
            Msg::Label(l) => WORDS.contains(l),
            Msg::FailWorkspace { workspace, why } => name.as_ref() == Some(workspace) && words.contains(why),
            _ => false,
        };
        self.status.restate_about(msg, level, about, now, stale);
        true
    }
}

/// The status line's words for live updates.
const WORDS: [Label; 4] = [Label::LiveOn, Label::LiveBack, Label::LiveLost, Label::LiveWaiting];
