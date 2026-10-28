//! Signing in and out where the Slack transport is not needed: the sign-in screen offered at
//! start (the Slack desktop app's workspaces first, else the paste screen) and `slakio logout`
//! (of one workspace, or of all).

use crate::cli::Which;
use crate::words::shown;
use slakio_auth::account;
use slakio_core::fault::ErrorLog;
use slakio_core::i18n::{self, Label, Msg};
use slakio_core::paths::Paths;
use slakio_core::secret::{Credentials, SecretStore, StoreKind};
use slakio_tui::app::App;
use slakio_tui::app::signin::Pasted;
use slakio_tui::app::signin::PickerRow;
use std::io::Read;
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

/// `slakio logout [<workspace> | --all]`. Signed in to one workspace (or with `--all`): every
/// one is signed out of ([`sign_out_all`]). Signed in to several, the one named is signed out of
/// alone — its credentials, its line of the workspaces file and its part of the cache — and
/// the others stay; with none named it lists them and asks for one (and the exit says failure).
pub(crate) fn sign_out(
    store: &dyn SecretStore,
    path: Option<&std::path::Path>,
    i18n: &i18n::I18n,
    paths: &Paths,
    which: &Which,
) -> ExitCode {
    let errors = || ErrorLog::new(paths.errors_log());
    let accounts = match path.map(account::load).transpose() {
        Ok(a) => a.unwrap_or_default(),
        // Unreadable: which one is meant cannot be told; signing out of all still removes the
        // cache whatever the file says.
        Err(_) if !matches!(which, Which::Named(_)) => return sign_out_all(store, path, i18n, paths),
        Err(fault) => {
            errors().record("logout", &fault);
            eprintln!("{}", i18n.label(Label::LogoutFailed));
            return ExitCode::FAILURE;
        }
    };
    let list = |of: &[&account::Account]| {
        eprintln!("{}", i18n.label(Label::LogoutChoose));
        for a in of {
            eprintln!("{}", i18n.msg(&Msg::LogoutListed { workspace: shown(&a.name), host: shown(&a.host) }));
        }
        ExitCode::FAILURE
    };
    let one = match which {
        Which::All => return sign_out_all(store, path, i18n, paths),
        _ if accounts.len() <= 1 && !matches!(which, Which::Named(_)) => return sign_out_all(store, path, i18n, paths),
        Which::Only => return list(&accounts.iter().collect::<Vec<_>>()),
        Which::Named(q) => match account::named(&accounts, q)[..] {
            [] => {
                eprintln!("{}", i18n.msg(&Msg::LogoutUnknown { workspace: shown(q) }));
                return ExitCode::FAILURE;
            }
            [a] => a.clone(),
            ref several => return list(several),
        },
    };
    if accounts.len() == 1 {
        return sign_out_all(store, path, i18n, paths);
    }
    sign_out_one(store, path, i18n, paths, &one)
}

/// Sign out of `a` alone (others stay signed in): its credentials and line first, then its part
/// of the cache. Not while a slakio runs and writes the cache: nothing is changed then, and the
/// user is told to quit it first.
fn sign_out_one(
    store: &dyn SecretStore,
    path: Option<&std::path::Path>,
    i18n: &i18n::I18n,
    paths: &Paths,
    a: &account::Account,
) -> ExitCode {
    let errors = || ErrorLog::new(paths.errors_log());
    let Some(path) = path else { return ExitCode::FAILURE };
    // A slakio that runs writes the cache: nothing is signed out until it is quit, so its part
    // of the cache is never left behind without a workspace that names it.
    if crate::store::in_use(paths) {
        eprintln!("{}", i18n.label(Label::LogoutStillRunning));
        return ExitCode::FAILURE;
    }
    if let Err(fault) = account::forget_one(store, path, &a.team) {
        errors().record("logout", &fault);
        eprintln!("{}", i18n.label(Label::LogoutFailed));
        return ExitCode::FAILURE;
    }
    let workspace = shown(&a.name);
    match crate::store::forget(paths, &a.team) {
        Ok(()) => {
            println!("{}", i18n.msg(&Msg::LogoutDoneOne { workspace }));
            ExitCode::SUCCESS
        }
        Err(running) => {
            println!("{}", i18n.msg(&Msg::LogoutDoneCacheKept { workspace }));
            match running {
                None => eprintln!("{}", i18n.label(Label::LogoutStillRunning)),
                Some(fault) => {
                    errors().record("logout", &fault);
                    eprintln!("{}", i18n.label(Label::LogoutCacheFailed));
                }
            }
            ExitCode::FAILURE
        }
    }
}

/// `slakio logout --all` (or of the one workspace there is): every signed-in workspace's
/// credentials leave the store, then the file, and the local cache goes whether or not that
/// worked: nothing of the sign-in is left. What it says names the cache only when it is gone. A
/// slakio still running keeps showing what it showed until it is quit, so the user is told to
/// quit it (and the exit says failure).
fn sign_out_all(store: &dyn SecretStore, path: Option<&std::path::Path>, i18n: &i18n::I18n, paths: &Paths) -> ExitCode {
    let errors = || ErrorLog::new(paths.errors_log());
    let forgot = path.map(|p| account::forget(store, p));
    let running = crate::store::in_use(paths);
    let cache = crate::store::remove(paths);
    let gone = cache.is_ok();
    let mut ok = gone;
    match forgot {
        None => println!("{}", i18n.label(if gone { Label::LogoutNone } else { Label::LogoutNoneCacheKept })),
        Some(Ok(names)) if names.is_empty() => {
            println!("{}", i18n.label(if gone { Label::LogoutNone } else { Label::LogoutNoneCacheKept }));
        }
        Some(Ok(names)) => {
            for a in names {
                let workspace = shown(&a.name);
                let msg = if gone { Msg::LogoutDone { workspace } } else { Msg::LogoutDoneCacheKept { workspace } };
                println!("{}", i18n.msg(&msg));
            }
        }
        Some(Err(fault)) => {
            errors().record("logout", &fault);
            eprintln!("{}", i18n.label(Label::LogoutFailed));
            ok = false;
        }
    }
    if let Err(fault) = cache {
        errors().record("logout", &fault);
        eprintln!("{}", i18n.label(Label::LogoutCacheFailed));
    }
    if running {
        eprintln!("{}", i18n.label(Label::LogoutStillRunning));
        ok = false;
    }
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// `SLAKIO_TEST_CREDENTIALS` (`<team>|<token>|<cookie>`, several separated by `;`), debug builds
/// with the memory store only: the tests start the binary signed in, as keychain entries from an
/// earlier run would. A release build, or the real keychain, never reads it.
pub(crate) fn test_credentials(store: &dyn SecretStore, kind: StoreKind) {
    if !cfg!(debug_assertions) || kind != StoreKind::Memory {
        return;
    }
    let Ok(value) = std::env::var("SLAKIO_TEST_CREDENTIALS") else { return };
    for entry in value.split(';') {
        if let [team, token, cookie] = entry.splitn(3, '|').collect::<Vec<_>>()[..]
            && account::valid_team(team)
        {
            let _ = store.set(team, &account::encode(&Credentials::new(token, cookie)));
        }
    }
}

/// `slakio login` with the paste on standard input: the workspace it signs in to is added to
/// those signed in to (or updated, when it is one of them).
pub(crate) fn sign_in_piped(wiring: &crate::Wiring, i18n: &i18n::I18n) -> ExitCode {
    let mut text = Pasted(String::new());
    let read = std::io::stdin().take(slakio_auth::curl::PASTE_MAX as u64 + 1).read_to_string(&mut text.0);
    let result = match read {
        Err(_) => Err(Label::SigninEmpty),
        Ok(_) => match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt.block_on(wiring.clone().sign_in(text)),
            Err(_) => Err(Label::SigninFailed),
        },
    };
    match result {
        Ok(s) => {
            println!("{}", i18n.msg(&Msg::SigninSignedIn { workspace: shown(&s.account.name) }));
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("{}", i18n.label(why));
            ExitCode::FAILURE
        }
    }
}
