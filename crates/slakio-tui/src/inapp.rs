//! Signing in from the app, before the first workspace or while it runs (`:login`): the sign-in
//! is read, checked with Slack and kept ([`crate::Wiring::finish`]), then the app is connected.
//! A sign-in to a workspace the app serves already (signed out by Slack, or signed in again
//! anyway) starts only that workspace's backend again — its live updates with it, its part of
//! the cache claimed again (kept for the same user, emptied for another) — and every other
//! workspace goes on as it was. A sign-in to another workspace connects them all again.

use crate::{Kept, Wiring};
use slakio_core::backend::BackendHandle;
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::WorkspaceId;
use slakio_tui::app::App;
use slakio_tui::app::model::SignedIn as SignedInTo;
use slakio_tui::app::signin::Pasted;
use slakio_tui::app::status::Level;
use std::sync::{Arc, PoisonError};
use std::time::Instant;
use tokio::task::{JoinError, JoinHandle};

/// Signing in from the app ended: the workspaces it signed in to (their names), how the app is
/// connected now, and why one asked for did not sign in, if one did not.
pub(crate) struct Done {
    names: Vec<String>,
    /// The first workspace signed in to (its team id): the app shows it.
    added: Option<String>,
    connected: Connected,
    failed: Option<Label>,
}

/// How the app is connected after a sign-in.
enum Connected {
    /// Every signed-in workspace (those signed in to before too), as [`Wiring::connect`] gives
    /// them, with what to tell about the cache.
    All(BackendHandle, Vec<SignedInTo>, Vec<Msg>),
    /// Only these workspaces, served already, started again; the others untouched.
    Again(Vec<SignedInTo>, Vec<Msg>),
}

/// A sign-in being checked with Slack, then connected.
pub(crate) type Signing = Option<JoinHandle<Result<Done, Label>>>;

/// Check a sign-in (`task`) off the loop's thread: one at a time (the app asks for none while
/// one is checked), so one before, should there be one, is given up.
pub(crate) fn start(signing_in: &mut Signing, task: impl Future<Output = Result<Done, Label>> + Send + 'static) {
    if let Some(earlier) = signing_in.replace(tokio::spawn(task)) {
        earlier.abort();
    }
}

/// The desktop app's workspaces being read for `:login` (off the loop's thread), and the team
/// the picker's cursor goes on.
pub(crate) type Scan = (JoinHandle<Result<slakio_auth::Found, slakio_auth::DesktopError>>, Option<String>);

/// `:login`: the sign-in screen over the app, as at the start — the paste screen at once when
/// the desktop app's workspaces may not be read; else they are read first ([`Scan`]: the files
/// are read off the loop's thread, which goes on drawing), then offered ([`scanned`]).
pub(crate) fn offer(app: &mut App, wiring: &Wiring, team: Option<String>) -> Option<Scan> {
    if !wiring.desktop_ok {
        app.offer_sign_in(None);
        return None;
    }
    Some((tokio::task::spawn_blocking(slakio_auth::desktop::scan), team))
}

/// The desktop scan of `:login` is done: its workspaces as a picker, the cursor on `team`'s (or
/// the paste screen, with why).
pub(crate) fn scanned(app: &mut App, wiring: &mut Wiring, done: Result<ScanResult, JoinError>, team: Option<&str>) {
    wiring.desktop = match done {
        Ok(found) => crate::signin::show_scan(app, found).map(Arc::new),
        Err(_) => {
            app.offer_sign_in(Some(Label::SigninDesktopFailed));
            None
        }
    };
    if let Some(team) = team {
        app.sign_in_select(team);
    }
}

type ScanResult = Result<slakio_auth::Found, slakio_auth::DesktopError>;

impl Wiring {
    /// Sign in with a pasted request ([`Wiring::sign_in`]), then connect.
    pub(crate) async fn sign_in_app(self, text: Pasted) -> Result<Done, Label> {
        let one = self.clone().sign_in(text).await?;
        let (names, added) = (vec![crate::words::shown(&one.account.name)], Some(one.account.team.clone()));
        let connected = self.reconnect(vec![one]).await?;
        Ok(Done { names, added, connected, failed: None })
    }

    /// Sign in with each of the picked desktop workspaces `teams`, one after the other; one that
    /// does not sign in holds none of the others back (the first reason is kept). Then connect.
    pub(crate) async fn sign_in_desktop_all(self, teams: Vec<String>) -> Result<Done, Label> {
        let (mut names, mut added, mut failed, mut kept) = (Vec::new(), None, None, Vec::new());
        for team in teams {
            match self.clone().sign_in_desktop(team).await {
                Ok(k) => {
                    names.push(crate::words::shown(&k.account.name));
                    added.get_or_insert(k.account.team.clone());
                    kept.push(k);
                }
                Err(why) => {
                    failed.get_or_insert(why);
                }
            }
        }
        if names.is_empty() {
            return Err(failed.unwrap_or(Label::SigninDesktopFailed));
        }
        let connected = self.reconnect(kept).await?;
        Ok(Done { names, added, connected, failed })
    }

    /// Connect after signing in to `kept`: when the app serves each of them already, only their
    /// backends start again (off the loop's thread: claiming a part of the cache may wait);
    /// otherwise every signed-in workspace is connected again.
    async fn reconnect(&self, kept: Vec<Kept>) -> Result<Connected, Label> {
        let replace = self.replace.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let served = replace.filter(|r| kept.iter().all(|k| r.serves(&WorkspaceId::new(&k.account.team))));
        let Some(replace) = served else {
            let (handle, signed, notices) = self.connect_all().await?;
            return Ok(Connected::All(handle, signed, notices));
        };
        let w = self.clone();
        let again = tokio::task::spawn_blocking(move || {
            let (mut signed, mut notices) = (Vec::new(), Vec::new());
            for k in kept {
                let id = WorkspaceId::new(&k.account.team);
                let (team, user) = (k.account.team.clone(), k.account.user.clone());
                let handle = w.local.front(w.serve(k.credentials, k.endpoints, &team), &team, &user, &mut notices);
                let read_only = handle.capabilities.read_only;
                if replace.put(&id, handle) {
                    signed.push(SignedInTo { id, name: k.account.name, read_only, user });
                }
            }
            (signed, notices)
        });
        let (signed, notices) = again.await.map_err(|_| Label::SigninFailed)?;
        Ok(Connected::Again(signed, notices))
    }
}

/// Signing in from the app ended: with a sign-in the app connects (every signed-in workspace,
/// each booted again; or only those signed in to again), else says why not.
pub(crate) fn signed_in(done: Result<Done, Label>, app: &mut App, backend: &mut Option<BackendHandle>) {
    let d = match done {
        Ok(d) => d,
        Err(why) => return app.sign_in_failed(why),
    };
    let mut notices = match d.connected {
        Connected::All(handle, signed, notices) => {
            let added = d.added.map(WorkspaceId::new);
            app.signed_in(handle.capabilities, &signed, added.as_ref(), &d.names, Instant::now());
            *backend = Some(handle);
            notices
        }
        Connected::Again(signed, notices) => {
            app.signed_in_again(&signed, &d.names, Instant::now());
            notices
        }
    };
    if let Some(why) = d.failed {
        notices.insert(0, Msg::Label(why));
    }
    // Queued after "Signed in to …", which stays readable.
    for n in notices {
        app.status.queue(n, Level::Warning, Instant::now());
    }
}
