//! One workspace's client: every call passes the allowlist, then the process's rails
//! ([`Governor`], through this workspace's own lane of it), then goes out in the web client's format and comes back decoded.

use crate::error::SlackError;
use crate::governor::{Governor, Lane, Priority};
use crate::method::Method;
use crate::request::{Call, Endpoints, Ids, Os, RandomIds, Session, opening, prepare};
use crate::response::decode;
use serde::de::DeserializeOwned;
use slakio_core::secret::Credentials;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub struct Client {
    credentials: Credentials,
    endpoints: Endpoints,
    governor: Arc<Governor>,
    /// This workspace's share of the governor's slots.
    lane: Lane,
    state: Mutex<(Session, RandomIds)>,
}

impl Client {
    pub fn new(credentials: Credentials, endpoints: Endpoints, governor: Arc<Governor>) -> Self {
        let mut ids = RandomIds::default();
        let session = Session::new(&mut ids);
        let lane = governor.lane();
        Self { credentials, endpoints, governor, lane, state: Mutex::new((session, ids)) }
    }

    /// Signed in to `team`: later requests carry it.
    pub fn set_team(&self, team: &str) {
        self.state.lock().unwrap_or_else(PoisonError::into_inner).0.set_team(team);
    }

    /// Call `call` and decode its answer as `T`. With the network off, or the cap reached,
    /// nothing is sent; switching the network off while it is in flight ends it.
    pub async fn call<T: DeserializeOwned>(&self, call: &Call, priority: Priority) -> Result<T, SlackError> {
        let _slot = self.governor.admit_in(&self.lane, call.method, priority).await?;
        let prepared = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let (session, ids) = &mut *state;
            let ids: &mut dyn Ids = ids;
            prepare(call, &self.credentials, session, &self.endpoints, now_ms(), ids, Os::current())?
        };
        // The switch is looked at first: once it is off, nothing more of the request is written.
        let raw = tokio::select! {
            biased;
            () = self.governor.stopped() => return Err(SlackError::Offline { why: "the network was switched off" }),
            raw = crate::http::send(&self.endpoints, &prepared) => raw?,
        };
        let answer = decode(&raw);
        if let Err(SlackError::RateLimited { retry_after }) = &answer {
            self.governor.pause(call.method, *retry_after);
        }
        answer
    }

    /// Open the live updates' socket: past the kill switch and the cap (an opening counts as
    /// one request), to the realtime host only. Switching the network off while it opens ends it.
    pub(crate) async fn open_socket(&self) -> Result<crate::http::Upgraded, SlackError> {
        self.governor.dial(&self.lane)?;
        let opening = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let (session, ids) = &mut *state;
            opening(&self.credentials, session, &self.endpoints, ids, Os::current())?
        };
        tokio::select! {
            biased;
            () = self.governor.stopped() => Err(SlackError::Offline { why: "the network was switched off" }),
            socket = crate::http::socket(&self.endpoints, &opening) => socket,
        }
    }

    /// The process's rails.
    pub(crate) fn governor(&self) -> &Governor {
        &self.governor
    }

    /// Call the method named `name` with form parameters `params`: a name that is not on the
    /// allowlist is refused before anything is counted or sent.
    pub async fn call_named(
        &self,
        name: &str,
        params: &[(&str, &str)],
        priority: Priority,
    ) -> Result<serde_json::Value, SlackError> {
        let method = Method::parse(name)?;
        self.call(&Call::form(method, params), priority).await
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
