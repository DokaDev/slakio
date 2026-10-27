//! Signing in and out where the Slack transport is not needed: the sign-in screen offered at
//! start (the Slack desktop app's workspaces first, else the paste screen) and `slakio logout`.

use crate::words::shown;
use slakio_auth::account;
use slakio_core::fault::ErrorLog;
use slakio_core::i18n::{self, Label, Msg};
use slakio_core::paths::Paths;
use slakio_core::secret::{Credentials, SecretStore, StoreKind};
use slakio_tui::app::App;
use slakio_tui::app::signin::PickerRow;
use std::process::ExitCode;

/// Offer signing in: when the real keychain is in use and nothing failed yet, try the Slack
/// desktop app first and show its workspaces as a picker; otherwise (or when it finds nothing)
/// fall back to the paste screen, with a short reason.
pub(crate) fn offer_sign_in_screen(app: &mut App, desktop_ok: bool, why: Option<Label>) -> Option<slakio_auth::Found> {
    use slakio_auth::DesktopError;
    if why.is_some() || !desktop_ok {
        app.offer_sign_in(why);
        return None;
    }
    // Before the terminal is set up: the scan reads files only (no keychain, no network).
    match slakio_auth::desktop::scan() {
        Ok(found) => {
            app.offer_desktop(
                found
                    .workspaces()
                    .iter()
                    .map(|w| PickerRow { team: w.team.clone(), name: shown(&w.name), host: shown(&w.host) })
                    .collect(),
            );
            return Some(found);
        }
        Err(DesktopError::NotSupported) => app.offer_sign_in(Some(Label::SigninDesktopUnsupported)),
        Err(e) if e.is_absent() => app.offer_sign_in(Some(Label::SigninDesktopMissing)),
        Err(_) => app.offer_sign_in(Some(Label::SigninDesktopFailed)),
    }
    None
}

/// `slakio logout`: every signed-in workspace's credentials leave the store, then the file, then
/// the local cache: nothing of the sign-in is left.
pub(crate) fn sign_out(
    store: &dyn SecretStore,
    path: Option<&std::path::Path>,
    i18n: &i18n::I18n,
    paths: &Paths,
) -> ExitCode {
    let errors = || ErrorLog::new(paths.errors_log());
    let Some(path) = path else {
        println!("{}", i18n.label(Label::LogoutNone));
        return ExitCode::SUCCESS;
    };
    match account::forget(store, path) {
        Ok(gone) if gone.is_empty() => println!("{}", i18n.label(Label::LogoutNone)),
        Ok(gone) => {
            for a in gone {
                println!("{}", i18n.msg(&Msg::LogoutDone { workspace: shown(&a.name) }));
            }
        }
        Err(fault) => {
            errors().record("logout", &fault);
            eprintln!("{}", i18n.label(Label::LogoutFailed));
            return ExitCode::FAILURE;
        }
    }
    if let Err(fault) = crate::store::remove(paths) {
        errors().record("logout", &fault);
        eprintln!("{}", i18n.label(Label::LogoutCacheFailed));
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// `SLAKIO_TEST_CREDENTIALS` (`<team>|<token>|<cookie>`), debug builds with the memory store
/// only: the tests start the binary signed in, as a keychain entry from an earlier run would.
/// A release build, or the real keychain, never reads it.
pub(crate) fn test_credentials(store: &dyn SecretStore, kind: StoreKind) {
    if !cfg!(debug_assertions) || kind != StoreKind::Memory {
        return;
    }
    let Ok(value) = std::env::var("SLAKIO_TEST_CREDENTIALS") else { return };
    if let [team, token, cookie] = value.splitn(3, '|').collect::<Vec<_>>()[..]
        && account::valid_team(team)
    {
        let _ = store.set(team, &account::encode(&Credentials::new(token, cookie)));
    }
}
