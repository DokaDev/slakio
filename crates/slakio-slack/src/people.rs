//! The people a workspace's messages and DMs name, looked up by id and kept for the session (in
//! memory): never a directory listing. What a view brings along is kept as it comes; the rest
//! is asked of the edge cache's `users/info`, [`LOOKUP_BATCH`] ids a request (the web client's
//! batch), each id once. A lookup that fails is not kept, so the id shows until a later page
//! asks again; unknown is not absent.

use crate::client::Client;
use crate::governor::Priority;
use crate::method::Method;
use crate::request::Call;
use crate::wire;
use serde_json::{Map, Value, json};
use slakio_core::model::{Org, Presence, User, UserId, WorkspaceId};
use slakio_core::sanitize::Remote;
use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

/// How many people one edge lookup asks for.
pub const LOOKUP_BATCH: usize = 80;

/// The people known this session, and the team they were looked up for.
#[derive(Debug, Default)]
pub(crate) struct People {
    known: Mutex<HashMap<String, User>>,
    team: Mutex<Option<String>>,
}

impl People {
    /// Signed in to `team`: lookups are for it.
    pub(crate) fn set_team(&self, team: &str) {
        *self.team.lock().unwrap_or_else(PoisonError::into_inner) = Some(team.to_string());
    }

    fn team(&self) -> Option<String> {
        self.team.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Keep what Slack sent of `persons` (a view's authors).
    pub(crate) fn learn(&self, persons: &[wire::Person]) {
        let Some(team) = self.team() else { return };
        let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        for p in persons {
            if let Some(u) = user(p, &team) {
                known.insert(p.id.clone(), u);
            }
        }
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
            wanted.iter().copied().filter(|id| !known.contains_key(*id)).collect()
        };
        for batch in missing.chunks(LOOKUP_BATCH) {
            let updated: Map<String, Value> = batch.iter().map(|id| ((*id).clone(), json!(0))).collect();
            let mut body = Map::new();
            body.insert("check_interaction".into(), json!(true));
            body.insert("include_profile_only_if_stale".into(), json!(true));
            body.insert("updated_ids".into(), Value::Object(updated));
            let Ok(found) = client.call::<wire::EdgeUsers>(&Call::json(Method::EdgeUsersInfo, body), priority).await
            else {
                continue;
            };
            let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
            for p in found.results.unwrap_or_default() {
                if let Some(u) = user(&p, &team) {
                    known.insert(p.id.clone(), u);
                }
            }
        }
        let known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        wanted.iter().filter_map(|id| known.get(*id).cloned()).collect()
    }
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
    })
}

#[cfg(test)]
mod tests;
