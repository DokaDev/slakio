//! Signing in from the app. `slakio login` (and the first run without a workspace) first offers
//! the workspaces the Slack desktop app is signed in to, as a picker; the chosen one goes to the
//! binary by team id ([`Effect::DesktopPick`]), which reads its credentials (the macOS keychain
//! may prompt), asks Slack whether they sign in and keeps them. Pressing `p` (or no desktop app) falls back to
//! the paste flow: the screen says how to copy a request, what is pasted there goes to the binary
//! ([`Effect::SignIn`]). The pasted text is never kept by the app, drawn or printed.

use super::{App, Effect};
use crate::action::SignInAction;
use crate::keymap::Ctx;
use slakio_core::backend::Capabilities;
use slakio_core::i18n::{Label, Msg};
use std::time::Instant;

/// Where signing in stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignIn {
    /// The desktop app is signed in to these workspaces; the user picks one (or presses `p` to
    /// paste a browser request instead).
    Desktop(Picker),
    /// A picked desktop workspace is being read and checked.
    DesktopChecking(Picker),
    /// Waiting for a paste.
    Waiting,
    /// A paste is being checked.
    Checking,
    /// The last paste did not sign in, for this reason; another may.
    Failed(Label),
}

/// One workspace of the desktop picker: its team id, its name and its host (Slack's text,
/// already sanitised). The host tells apart two workspaces of the same name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerRow {
    pub team: String,
    pub name: String,
    pub host: String,
}

/// The desktop-workspace picker: its rows, the one under the cursor, and the reason the last pick
/// did not sign in, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picker {
    pub rows: Vec<PickerRow>,
    pub selected: usize,
    pub note: Option<Label>,
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
    /// No workspace is signed in: the screen offers the paste flow, with `why` the last attempt
    /// (or reading the keychain) failed, if it did.
    pub fn offer_sign_in(&mut self, why: Option<Label>) {
        self.sign_in = Some(why.map_or(SignIn::Waiting, SignIn::Failed));
    }

    /// Offer the desktop app's signed-in workspaces as a picker. The pick is reported by team id.
    pub fn offer_desktop(&mut self, rows: Vec<PickerRow>) {
        self.sign_in = Some(SignIn::Desktop(Picker { rows, selected: 0, note: None }));
    }

    /// Where signing in stands; `None` when the app does not offer it.
    pub fn sign_in(&self) -> Option<SignIn> {
        self.sign_in.clone()
    }

    /// The paste being checked did not sign in, for `why`.
    pub fn sign_in_failed(&mut self, why: Label) {
        match &self.sign_in {
            // A desktop pick that failed returns to the picker with the reason shown.
            Some(SignIn::DesktopChecking(p)) => {
                let mut p = p.clone();
                p.note = Some(why);
                self.sign_in = Some(SignIn::Desktop(p));
            }
            Some(_) => self.sign_in = Some(SignIn::Failed(why)),
            None => {}
        }
    }

    /// Signed in to `workspace` (Slack's name, already sanitised): the app connects to the
    /// backend that can do `caps`.
    pub fn signed_in(&mut self, caps: Capabilities, workspace: String, now: Instant) {
        self.sign_in = None;
        self.connect(caps);
        self.info(Msg::SigninSignedIn { workspace }, now);
    }

    /// The desktop picker has the keyboard (its keys are [`Ctx::SignIn`]'s).
    pub(super) fn picking(&self) -> bool {
        matches!(self.sign_in, Some(SignIn::Desktop(_)))
    }

    /// An action of the desktop picker: move, pick (the binary reads and checks that workspace),
    /// or switch to the paste flow.
    pub(super) fn sign_in_action(&mut self, a: SignInAction) {
        let Some(SignIn::Desktop(p)) = &mut self.sign_in else { return };
        match a {
            SignInAction::Down => p.selected = (p.selected + 1).min(p.rows.len().saturating_sub(1)),
            SignInAction::Up => p.selected = p.selected.saturating_sub(1),
            SignInAction::Pick => {
                let Some(team) = p.rows.get(p.selected).map(|r| r.team.clone()) else { return };
                let p = Picker { note: None, ..p.clone() };
                self.sign_in = Some(SignIn::DesktopChecking(p));
                self.effects.push(Effect::DesktopPick(team));
            }
            SignInAction::Paste => self.offer_sign_in(None),
        }
    }

    /// Text pasted (bracketed paste) where the keyboard is: a sign-in on the paste screen or the
    /// desktop picker (a request copied first is taken there too), else typed into the composer,
    /// the help filter or the command line.
    pub(super) fn paste(&mut self, text: String) {
        match self.key_context() {
            Ctx::SignIn => {
                self.sign_in = Some(SignIn::Checking);
                self.effects.push(Effect::SignIn(Pasted(text)));
            }
            Ctx::Root if matches!(self.sign_in, Some(SignIn::Waiting | SignIn::Failed(_))) => {
                self.sign_in = Some(SignIn::Checking);
                self.effects.push(Effect::SignIn(Pasted(text)));
            }
            // Checking: one sign-in at a time.
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
