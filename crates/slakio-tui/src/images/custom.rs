//! A workspace's own emoji, as the backend read them (`emoji.list`, [`Command::Emoji`]): what
//! each name draws, kept in memory for the run. A workspace's list is asked for once, after the
//! first frame that shows a shortcode the standard emoji do not know (a workspace whose messages
//! use none asks for nothing) once that workspace has booted ([`Customs::booted_only`]), and
//! again once it is signed in again ([`Customs::forget`]). One that failed for the network, the
//! request budget or one of Slack's passing errors ([`PASSING`]) is asked again, when such a
//! shortcode shows again, after [`super::RETRY_AFTER`] ([`super::RETRY_SOON`] for a busy or
//! offline backend, the pause Slack asked for when longer), each failure in a row twice as long,
//! at most [`BACKOFF_MAX`]; a passing error at most [`PASSING_TRIES`] times in a row (a busy or
//! offline backend asked nothing of Slack, so it counts toward neither). Any other
//! failure (a refused sign-in, a missing scope) leaves the shortcodes as text for the run.
//! Nothing of it is ever said in the status line: until a list comes, `:name:` stays text.

use crate::app::requests::Requests;
use slakio_core::backend::{Command, FailKind, Generation};
use slakio_core::diag::{Diag, Kind, Stage};
use slakio_core::images::emoji::Custom;
use slakio_core::model::WorkspaceId;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Slack's error codes that pass (its own trouble, an answer that was not its JSON): asked again.
pub const PASSING: &[&str] =
    &["internal_error", "fatal_error", "request_timeout", "service_unavailable", "not_slack_json"];

/// How many passing errors in a row a workspace's list is asked again after.
pub const PASSING_TRIES: u32 = 3;

/// The longest wait before a failed list is asked again.
pub const BACKOFF_MAX: Duration = Duration::from_secs(15 * 60);

/// The workspaces' own emoji.
#[derive(Debug, Default)]
pub struct Customs {
    /// What each name draws, by workspace (the list came).
    sets: HashMap<WorkspaceId, HashMap<String, Custom>>,
    asked: HashMap<Generation, WorkspaceId>,
    /// The workspaces whose list failed, and when it may be asked for again (never: `None`).
    failed: HashMap<WorkspaceId, Option<Instant>>,
    /// How many times in a row each workspace's list failed (busy or offline not counted).
    tries: HashMap<WorkspaceId, u32>,
    /// The workspaces a frame showed a shortcode of that no list says yet (`false`: only laid
    /// out, not shown).
    seen: RefCell<Vec<(WorkspaceId, bool)>>,
    /// The picture diagnostics (off unless asked for).
    pub diag: Diag,
}

impl Customs {
    /// What `name` (a shortcode the standard emoji do not know) draws in workspace `ws`. While
    /// the workspace's list has not come, nothing; the frame notes that one is wanted (a name of
    /// digits only, as `10:30:` reads, is no emoji's; a letter of any script counts, Hangul too).
    pub fn get(&self, ws: &WorkspaceId, name: &str) -> Option<&Custom> {
        match self.sets.get(ws) {
            Some(set) => {
                let got = set.get(name);
                let lettered = || name.chars().any(char::is_alphabetic);
                if got.is_none() && lettered() && self.diag.first(Kind::EmojiList, Stage::Missing, ws.as_str()) {
                    self.note(ws, Stage::Missing, "ui", &[]);
                }
                got
            }
            None => {
                if name.chars().any(char::is_alphabetic) {
                    self.seen.borrow_mut().push((ws.clone(), true));
                    if self.diag.first(Kind::EmojiList, Stage::Wanted, ws.as_str()) {
                        self.note(ws, Stage::Wanted, "ui", &[]);
                    }
                }
                None
            }
        }
    }

    /// Where what frames note stands now ([`super::Images::mark`]).
    pub fn mark(&self) -> usize {
        self.seen.borrow().len()
    }

    /// What was noted between `from` and `to` was only laid out ([`super::Images::not_shown`]).
    pub fn not_shown(&self, from: usize, to: usize) {
        for s in self.seen.borrow_mut().get_mut(from..to).into_iter().flatten() {
            s.1 = false;
        }
    }

    /// Before [`Customs::ask`]: what a frame wanted of a workspace that has not booted yet (shown
    /// from the local cache) is let go, so its list is asked only with the request the booted
    /// workspace sends (the next frame after the boot wants it again).
    pub fn booted_only(&mut self, booted: impl Fn(&WorkspaceId) -> bool) {
        let seen = self.seen.get_mut();
        let before = seen.len();
        let mut waiting = Vec::new();
        seen.retain(|(ws, shown)| {
            let up = booted(ws);
            if !up && *shown && !waiting.contains(ws) {
                waiting.push(ws.clone());
            }
            up
        });
        if seen.len() == before {
            return;
        }
        for ws in waiting {
            if self.diag.first(Kind::EmojiList, Stage::Skipped, &format!("boot{}", ws.as_str())) {
                self.note(&ws, Stage::Skipped, "not-booted", &[]);
            }
        }
    }

    /// After a frame: the lists it wanted are asked for, each workspace's once (a failed one once
    /// its wait is over). `true` when something was asked.
    pub fn ask(&mut self, requests: &mut Requests, now: Instant) -> bool {
        let mut asked = false;
        for (ws, _) in self.seen.take().into_iter().filter(|s| s.1) {
            let waiting = self.asked.values().any(|w| *w == ws);
            let barred = self.failed.get(&ws).is_some_and(|again| again.is_none_or(|at| at > now));
            if self.sets.contains_key(&ws) || waiting || barred {
                if barred && self.diag.first(Kind::EmojiList, Stage::Skipped, ws.as_str()) {
                    let again = self.failed.get(&ws).copied().flatten();
                    self.note(&ws, Stage::Skipped, "barred", &[("retry", &super::note::retry(again, now))]);
                }
                continue;
            }
            self.failed.remove(&ws);
            self.note(&ws, Stage::Asked, "ui", &[]);
            let id = requests.ask(Some(ws.clone()), Command::Emoji);
            self.asked.insert(id, ws);
            asked = true;
        }
        asked
    }

    /// List request `id` came with `emoji`: `true` when it was one of these.
    pub fn answered(&mut self, id: Generation, emoji: Vec<(String, Custom)>) -> bool {
        let Some(ws) = self.asked.remove(&id) else { return false };
        if self.diag.on() {
            let pictures = emoji.iter().filter(|(_, c)| matches!(c, Custom::Picture(_))).count().to_string();
            self.note(&ws, Stage::Applied, "ok", &[("names", &emoji.len().to_string()), ("pictures", &pictures)]);
        }
        self.tries.remove(&ws);
        self.sets.insert(ws, emoji.into_iter().collect());
        true
    }

    /// List request `id` failed (`kind`) at `now`: `true` when it was one of these.
    pub fn failed(&mut self, id: Generation, kind: &FailKind, now: Instant) -> bool {
        let Some(ws) = self.asked.remove(&id) else { return false };
        let tries = self.tries.entry(ws.clone()).or_insert(0);
        // A busy or offline backend never asked Slack: no try of the list.
        if !matches!(kind, FailKind::Busy | FailKind::Offline) {
            *tries = tries.saturating_add(1);
        }
        let backoff = |base: Duration| base.saturating_mul(1 << tries.saturating_sub(1).min(16)).min(BACKOFF_MAX);
        let wait = match kind {
            FailKind::Busy | FailKind::Offline => Some(super::RETRY_SOON),
            FailKind::RateLimited { retry_after } => {
                Some(backoff(super::RETRY_AFTER).max(retry_after.unwrap_or_default().min(BACKOFF_MAX)))
            }
            FailKind::Api { code } if PASSING.contains(&code.as_str()) && *tries < PASSING_TRIES => {
                Some(backoff(super::RETRY_AFTER))
            }
            k if k.retryable() || *k == FailKind::CapReached => Some(backoff(super::RETRY_AFTER)),
            _ => None,
        };
        let again = wait.map(|w| now + w);
        self.note(&ws, Stage::Failed, &super::note::fail_word(kind), &[("retry", &super::note::retry(again, now))]);
        self.failed.insert(ws, again);
        true
    }

    /// When each failed list may be asked again.
    pub(super) fn retry_times(&self) -> impl Iterator<Item = Instant> + '_ {
        self.failed.values().filter_map(|again| *again)
    }

    /// One diagnostics line about workspace `ws`'s list.
    fn note(&self, ws: &WorkspaceId, stage: Stage, result: &str, pairs: &[(&str, &str)]) {
        if !self.diag.on() {
            return;
        }
        let id = self.diag.id(ws.as_str());
        let mut all = vec![("ws", id.as_str())];
        all.extend_from_slice(pairs);
        self.diag.line(Kind::EmojiList, stage, result, &all);
    }

    /// Workspace `ws` was signed in again: its list is read again when next wanted (one asked
    /// for before is not waited for: its answer is the old sign-in's).
    pub fn forget(&mut self, ws: &WorkspaceId) {
        self.sets.remove(ws);
        self.failed.remove(ws);
        self.tries.remove(ws);
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
