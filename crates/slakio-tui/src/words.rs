//! The binary's words for what it cannot do itself: a desktop import or a paste that cannot
//! sign in, a config file that cannot be used, a workspace's name made safe to print. Kept apart
//! from the wiring so the one file that names the Slack transport stays small.

use slakio_auth::PasteError;
use slakio_core::config::ConfigError;
use slakio_core::i18n::{Label, Msg};
use slakio_core::sanitize::Remote;

/// The words for a sign-in Slack did not accept, or could not be asked about.
pub(crate) fn check_label(e: &slakio_slack::SlackError) -> Label {
    use slakio_slack::SlackError;
    match e {
        SlackError::Auth { .. } => Label::SigninRefused,
        SlackError::Network { .. } => Label::SigninNetwork,
        SlackError::Offline { .. } => Label::SigninOffline,
        _ => Label::SigninFailed,
    }
}

/// The words for a desktop import that could not sign in.
pub(crate) fn desktop_label(e: slakio_auth::DesktopError) -> Label {
    use slakio_auth::DesktopError;
    match e {
        DesktopError::NotSupported => Label::SigninDesktopUnsupported,
        DesktopError::NoSecretService => Label::SigninDesktopKeychain,
        DesktopError::CookieLocked => Label::SigninDesktopLocked,
        _ if e.is_absent() => Label::SigninDesktopMissing,
        _ => Label::SigninDesktopFailed,
    }
}

/// The words for a paste that cannot sign in.
pub(crate) fn paste_label(e: PasteError) -> Label {
    match e {
        PasteError::Empty => Label::SigninEmpty,
        PasteError::TooLong => Label::SigninTooLong,
        PasteError::NoAddress => Label::SigninNoAddress,
        PasteError::NotWorkspace => Label::SigninNotWorkspace,
        PasteError::NoToken => Label::SigninNoToken,
        PasteError::NoCookie => Label::SigninNoCookie,
        PasteError::Ambiguous => Label::SigninAmbiguous,
    }
}

/// A workspace's name as Slack gave it, safe to print.
pub(crate) fn shown(name: &str) -> String {
    Remote::new(name).line().into_string()
}

/// The status line's words for a config file that cannot be used.
pub(crate) fn config_message(e: &ConfigError) -> Msg {
    match e {
        ConfigError::Read { .. } => Msg::Label(Label::ConfigReadFailed),
        ConfigError::Syntax(_) => Msg::Label(Label::ConfigSyntax),
        ConfigError::UnknownKey(key) => Msg::ConfigUnknownKey { key: key.clone() },
        ConfigError::Value { key, value, allowed } => {
            Msg::ConfigBadValue { key: key.clone(), value: value.clone(), allowed: allowed.clone() }
        }
    }
}
