//! The people a workspace's messages and DMs name, looked up by id and kept for the session (in
//! memory): never a directory listing. What a view brings along is kept as it comes; the rest is
//! asked of the edge cache's `users/info`, [`LOOKUP_BATCH`] ids a request (the web client's batch),
//! each id once, at version 0 and for the whole profile (its photo with it). A lookup that fails is
//! not kept, so the id shows until a later page asks again; unknown is not absent. An id Slack
//! answered for without a name to show is not asked again this session; one it left out, listed as
//! pending (not ready yet) or as failed (which may pass) is asked again by a later lookup, at most
//! [`OMITTED_MAX`] times. Only what a lookup asked for is taken from its answer.

use crate::client::Client;
use crate::governor::Priority;
use crate::method::Method;
use crate::request::Call;
use crate::wire;
use serde_json::{Map, Value, json};
use slakio_core::diag::Kind;
use slakio_core::model::{Org, Presence, User, UserId, WorkspaceId};
use slakio_core::sanitize::Remote;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, PoisonError};

/// How many people one edge lookup asks for.
pub const LOOKUP_BATCH: usize = 80;

/// How many lookups may leave an id out (or list it as pending or failed) before it is not asked
/// again.
pub const OMITTED_MAX: u8 = 3;

/// The people known this session, and the team they were looked up for.
#[derive(Debug, Default)]
pub(crate) struct People {
    known: Mutex<HashMap<String, User>>,
    /// Ids Slack was asked about and gave no name for.
    unnamed: Mutex<HashSet<String>>,
    /// Ids lookups left out or listed as pending or failed, and how many times.
    omitted: Mutex<HashMap<String, u8>>,
    /// Ids a lookup's answer had, and whether a photo was taken of them (for the diagnostics).
    answers: Mutex<HashMap<String, bool>>,
    team: Mutex<Option<String>>,
}

impl People {
    /// Signed in to `team`: lookups are for it.
    pub(crate) fn set_team(&self, team: &str) {
        *self.team.lock().unwrap_or_else(PoisonError::into_inner) = Some(team.to_string());
    }

    pub(crate) fn team(&self) -> Option<String> {
        self.team.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Keep what Slack sent of `persons` (a view's authors).
    pub(crate) fn learn(&self, persons: &[wire::Person]) {
        let Some(team) = self.team() else { return };
        let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        for p in persons {
            if let Some(u) = user(p, &team) {
                keep(&mut known, &p.id, u);
            }
        }
    }

    /// Forget person `id` when no photo of them is known, so the next [`People::names`] asks
    /// Slack for them again (the user's own record, when the start bundle has no photo): also
    /// when an earlier lookup gave no name for them.
    pub(crate) fn forget_unpictured(&self, id: &str) {
        let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        if known.get(id).is_none_or(|u| u.avatar.is_none()) {
            known.remove(id);
            self.unnamed.lock().unwrap_or_else(PoisonError::into_inner).remove(id);
            self.omitted.lock().unwrap_or_else(PoisonError::into_inner).remove(id);
        }
    }

    /// Person `id` as known now, when a photo of them is: nothing is asked.
    pub(crate) fn pictured(&self, id: &str) -> Option<User> {
        self.known.lock().unwrap_or_else(PoisonError::into_inner).get(id).filter(|u| u.avatar.is_some()).cloned()
    }

    /// Where person `id` stands before a lookup: `pictured` / `unpictured` (known, with a photo or
    /// without), `unnamed` (Slack gave no name for them: not asked again), or `unknown` (asked).
    pub(crate) fn standing(&self, id: &str) -> &'static str {
        if let Some(u) = self.known.lock().unwrap_or_else(PoisonError::into_inner).get(id) {
            return if u.avatar.is_some() { "pictured" } else { "unpictured" };
        }
        if self.unnamed.lock().unwrap_or_else(PoisonError::into_inner).contains(id) { "unnamed" } else { "unknown" }
    }

    /// Whether a lookup's answer had person `id` (and a photo of them was taken), since this was
    /// last asked: what it said is forgotten.
    pub(crate) fn answer(&self, id: &str) -> Option<bool> {
        self.answers.lock().unwrap_or_else(PoisonError::into_inner).remove(id)
    }

    /// The people of `ids` (in no order), those not known yet looked up first: in batches of
    /// [`LOOKUP_BATCH`], each id once. Those Slack does not name are left out.
    pub(crate) async fn names(&self, client: &Client, ids: &[String], priority: Priority) -> Vec<User> {
        let Some(team) = self.team() else { return Vec::new() };
        let mut wanted: Vec<&String> = ids.iter().collect();
        wanted.sort_unstable();
        wanted.dedup();
        let missing: Vec<&String> = {
            let known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
            let unnamed = self.unnamed.lock().unwrap_or_else(PoisonError::into_inner);
            wanted.iter().copied().filter(|id| !known.contains_key(*id) && !unnamed.contains(*id)).collect()
        };
        for batch in missing.chunks(LOOKUP_BATCH) {
            let updated: Map<String, Value> = batch.iter().map(|id| ((*id).clone(), json!(0))).collect();
            // No `include_profile_only_if_stale`: with it the edge cache may judge its record not
            // stale and answer without the profile's photo, though no photo of the id is held here.
            let mut body = Map::new();
            body.insert("check_interaction".into(), json!(true));
            body.insert("updated_ids".into(), Value::Object(updated));
            let found = match client.call::<wire::EdgeUsers>(&Call::json(Method::EdgeUsersInfo, body), priority).await {
                Ok(found) => found,
                Err(e) => {
                    let asked = batch.len().to_string();
                    crate::shape::failed(
                        client.diag(),
                        Kind::Avatar,
                        &e,
                        &[("lookup", "users-info"), ("asked", &asked)],
                    );
                    continue;
                }
            };
            crate::shape::photos(client.diag(), found.results.as_deref().unwrap_or_default(), "lookup");
            self.take(&team, batch, found);
        }
        let known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        wanted.iter().filter_map(|id| known.get(*id).cloned()).collect()
    }

    /// What one lookup of `batch` found (only the ids it asked for): the people named kept; an
    /// id answered without a name not asked again, unless a name of it is known; one left out,
    /// pending or failed asked again later, at most [`OMITTED_MAX`] lookups.
    fn take(&self, team: &str, batch: &[&String], found: wire::EdgeUsers) {
        let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        let mut unnamed = self.unnamed.lock().unwrap_or_else(PoisonError::into_inner);
        let mut omitted = self.omitted.lock().unwrap_or_else(PoisonError::into_inner);
        let mut answers = self.answers.lock().unwrap_or_else(PoisonError::into_inner);
        let asked: HashSet<&str> = batch.iter().map(|id| id.as_str()).collect();
        let mut answered = HashSet::new();
        for p in found.results.unwrap_or_default().into_iter().filter(|p| asked.contains(p.id.as_str())) {
            answered.insert(p.id.clone());
            answers.insert(p.id.clone(), p.profile.as_ref().and_then(wire::Profile::avatar).is_some());
            match user(&p, team) {
                Some(u) => keep(&mut known, &p.id, u),
                None if !known.contains_key(&p.id) => {
                    unnamed.insert(p.id.clone());
                }
                None => {}
            }
        }
        for id in batch.iter().filter(|id| !known.contains_key(**id) && !answered.contains(**id)) {
            let times = omitted.entry((*id).clone()).or_insert(0);
            *times += 1;
            if *times >= OMITTED_MAX {
                unnamed.insert((*id).clone());
            }
        }
    }
}

/// Keep `u`, replacing what was known of them; a photo known before stays when `u` has none
/// (a record without a photo does not say the photo is gone).
fn keep(known: &mut HashMap<String, User>, id: &str, mut u: User) {
    if u.avatar.is_none() {
        u.avatar = known.get(id).and_then(|old| old.avatar.clone());
    }
    known.insert(id.to_string(), u);
}

/// `p` as the core's person of `team`; `None` when Slack gave no name to show.
fn user(p: &wire::Person, team: &str) -> Option<User> {
    let shown = p.shown_name()?.to_string();
    let org = match p.team_id.as_deref() {
        // An empty team is unknown, not another organization.
        Some(t) if !t.is_empty() && t != team => Org::External(Remote::new(t)),
        _ => Org::Own,
    };
    Some(User {
        id: UserId::new(&p.id),
        workspace: WorkspaceId::new(team),
        name: Remote::new(p.name.clone().unwrap_or_else(|| shown.clone())),
        display_name: Remote::new(shown),
        org,
        bot: p.is_bot == Some(true),
        presence: Presence::Unknown,
        avatar: p.profile.as_ref().and_then(wire::Profile::avatar),
    })
}

#[cfg(test)]
mod tests;
