//! A workspace's own emoji, as the backend read them (`emoji.list`, [`Command::Emoji`]): what
//! each name draws, kept in memory for the run. A workspace's list is asked for once, after the
//! first frame that shows a shortcode the standard emoji do not know (a workspace whose messages
//! use none asks for nothing), and again once it is signed in again ([`Customs::forget`]). One
//! that failed for the network or the request budget is asked again after
//! [`super::RETRY_AFTER`] ([`super::RETRY_SOON`] for a busy or offline backend), when such a
//! shortcode shows again; any other failure leaves the workspace's shortcodes as text for the run.
//! Nothing of it is ever said in the status line: until a list comes, `:name:` stays text.

use crate::app::requests::Requests;
use slakio_core::backend::{Command, FailKind, Generation};
use slakio_core::images::emoji::Custom;
use slakio_core::model::WorkspaceId;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

/// The workspaces' own emoji.
#[derive(Debug, Default)]
pub struct Customs {
    /// What each name draws, by workspace (the list came).
    sets: HashMap<WorkspaceId, HashMap<String, Custom>>,
    asked: HashMap<Generation, WorkspaceId>,
    /// The workspaces whose list failed, and when it may be asked for again (never: `None`).
    failed: HashMap<WorkspaceId, Option<Instant>>,
    /// The workspaces a frame showed a shortcode of that no list says yet.
    seen: RefCell<Vec<WorkspaceId>>,
}

impl Customs {
    /// What `name` (a shortcode the standard emoji do not know) draws in workspace `ws`. While
    /// the workspace's list has not come, nothing; the frame notes that one is wanted (a name of
    /// digits only, as `10:30:` reads, is no emoji's).
    pub fn get(&self, ws: &WorkspaceId, name: &str) -> Option<&Custom> {
        match self.sets.get(ws) {
            Some(set) => set.get(name),
            None => {
                if name.bytes().any(|b| b.is_ascii_alphabetic()) {
                    self.seen.borrow_mut().push(ws.clone());
                }
                None
            }
        }
    }

    /// After a frame: the lists it wanted are asked for, each workspace's once (a failed one once
    /// its wait is over). `true` when something was asked.
    pub fn ask(&mut self, requests: &mut Requests, now: Instant) -> bool {
        let mut asked = false;
        for ws in self.seen.take() {
            let waiting = self.asked.values().any(|w| *w == ws);
            let barred = self.failed.get(&ws).is_some_and(|again| again.is_none_or(|at| at > now));
            if self.sets.contains_key(&ws) || waiting || barred {
                continue;
            }
            self.failed.remove(&ws);
            let id = requests.ask(Some(ws.clone()), Command::Emoji);
            self.asked.insert(id, ws);
            asked = true;
        }
        asked
    }

    /// List request `id` came with `emoji`: `true` when it was one of these.
    pub fn answered(&mut self, id: Generation, emoji: Vec<(String, Custom)>) -> bool {
        let Some(ws) = self.asked.remove(&id) else { return false };
        self.sets.insert(ws, emoji.into_iter().collect());
        true
    }

    /// List request `id` failed (`kind`) at `now`: `true` when it was one of these.
    pub fn failed(&mut self, id: Generation, kind: &FailKind, now: Instant) -> bool {
        let Some(ws) = self.asked.remove(&id) else { return false };
        let again = match kind {
            FailKind::Busy | FailKind::Offline => Some(now + super::RETRY_SOON),
            k if k.retryable() || *k == FailKind::CapReached => Some(now + super::RETRY_AFTER),
            _ => None,
        };
        self.failed.insert(ws, again);
        true
    }

    /// Workspace `ws` was signed in again: its list is read again when next wanted (one asked
    /// for before is not waited for: its answer is the old sign-in's).
    pub fn forget(&mut self, ws: &WorkspaceId) {
        self.sets.remove(ws);
        self.failed.remove(ws);
        self.asked.retain(|_, w| w != ws);
    }

    /// The picture addresses workspace `ws`'s list names.
    pub fn pictures(&self, ws: &WorkspaceId) -> Vec<String> {
        (self.sets.get(ws).into_iter().flat_map(HashMap::values))
            .filter_map(|c| match c {
                Custom::Picture(address) => Some(address.clone()),
                Custom::Standard(_) => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
