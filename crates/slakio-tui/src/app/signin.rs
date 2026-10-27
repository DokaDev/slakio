//! Signing in from the app. Without a workspace the screen says how to copy a request from the
//! browser ("Copy as cURL"); what is pasted there goes to the binary ([`Effect::SignIn`]), which
//! reads it, asks Slack whether it signs in and keeps it. Meanwhile the screen says it is
//! checking; afterwards the app connects, or the screen says why it did not sign in and takes
//! another paste. The pasted text is never kept by the app, drawn or printed.

use super::{App, Effect};
use crate::keymap::Ctx;
use slakio_core::backend::Capabilities;
use slakio_core::i18n::{Label, Msg};
use std::time::Instant;

/// Where signing in stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignIn {
    /// Waiting for a paste.
    Waiting,
    /// A paste is being checked.
    Checking,
    /// The last paste did not sign in, for this reason; another may.
    Failed(Label),
}

/// Pasted text that may hold a token and cookies: its `Debug` print says nothing of it, and
/// it is wiped from memory when dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct Pasted(pub String);

impl Drop for Pasted {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}

impl zeroize::ZeroizeOnDrop for Pasted {}

impl std::fmt::Debug for Pasted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pasted(<{} bytes>)", self.0.len())
    }
}

impl App {
    /// No workspace is signed in: the screen offers to sign in, with `why` the last attempt (or
    /// reading the keychain) failed, if it did.
    pub fn offer_sign_in(&mut self, why: Option<Label>) {
        self.sign_in = Some(why.map_or(SignIn::Waiting, SignIn::Failed));
    }

    /// Where signing in stands; `None` when the app does not offer it.
    pub fn sign_in(&self) -> Option<SignIn> {
        self.sign_in
    }

    /// The paste being checked did not sign in, for `why`.
    pub fn sign_in_failed(&mut self, why: Label) {
        if self.sign_in.is_some() {
            self.sign_in = Some(SignIn::Failed(why));
        }
    }

    /// Signed in to `workspace` (Slack's name, already sanitised): the app connects to the
    /// backend that can do `caps`.
    pub fn signed_in(&mut self, caps: Capabilities, workspace: String, now: Instant) {
        self.sign_in = None;
        self.connect(caps);
        self.info(Msg::SigninSignedIn { workspace }, now);
    }

    /// Text pasted (bracketed paste) where the keyboard is: a sign-in on the sign-in screen,
    /// else typed into the composer, the help filter or the command line.
    pub(super) fn paste(&mut self, text: String) {
        match self.key_context() {
            Ctx::Root if matches!(self.sign_in, Some(SignIn::Waiting | SignIn::Failed(_))) => {
                self.sign_in = Some(SignIn::Checking);
                self.effects.push(Effect::SignIn(Pasted(text)));
            }
            // Checking: one paste at a time.
            Ctx::Root if self.sign_in.is_some() => {}
            Ctx::ComposerInsert => {
                if let Some(c) = self.work.draft_mut() {
                    c.insert(&text);
                }
            }
            Ctx::HelpFilter => {
                if let Some(h) = self.help.as_mut() {
                    h.filter.extend(text.chars().filter(|c| !c.is_control()));
                    h.cursor = 0;
                }
            }
            _ => self.cmdline.insert(&text),
        }
    }
}
