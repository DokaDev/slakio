//! Signing in from the app. `slakio login` (and the first run without a workspace) first offers
//! the workspaces the Slack desktop app is signed in to, as a picker; the chosen one goes to the
//! binary ([`Effect::DesktopPick`], by team id), which reads its credentials (the macOS keychain may prompt),
//! asks Slack whether they sign in and keeps them. Pressing `p` (or no desktop app) falls back to
//! the paste flow: the screen says how to copy a request, what is pasted there goes to the binary
//! ([`Effect::SignIn`]). The pasted text is never kept by the app, drawn or printed.

use super::{App, Effect};
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

/// The desktop-workspace picker: the workspaces' team ids and names (Slack's text, already
/// sanitised), the one under the cursor, and the reason the last pick did not sign in, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picker {
    pub teams: Vec<String>,
    pub names: Vec<String>,
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

    /// Offer the desktop app's signed-in workspaces as a picker: `(team id, name)` pairs, the
    /// names Slack's text, already sanitised. The pick is reported by team id.
    pub fn offer_desktop(&mut self, workspaces: Vec<(String, String)>) {
        let (teams, names) = workspaces.into_iter().unzip();
        self.sign_in = Some(SignIn::Desktop(Picker { teams, names, selected: 0, note: None }));
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

    /// A key no binding claims on the sign-in screen: the desktop picker moves (`j`/`k`, arrows),
    /// picks (`Enter`) or switches to the paste flow (`p`); `true` when it was one of those. The
    /// paste screen takes a bracketed paste instead, handled in [`App::paste`].
    pub(super) fn sign_in_key(&mut self, key: crate::keymap::KeyChord) -> bool {
        use ratatui::crossterm::event::KeyCode;
        let Some(SignIn::Desktop(p)) = &mut self.sign_in else { return false };
        if !key.mods.is_empty() {
            return false;
        }
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if p.selected + 1 < p.names.len() {
                    p.selected += 1;
                }
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                p.selected = p.selected.saturating_sub(1);
                true
            }
            KeyCode::Enter => {
                let Some(team) = p.teams.get(p.selected).cloned() else { return false };
                let p = p.clone();
                self.sign_in = Some(SignIn::DesktopChecking(p));
                self.effects.push(Effect::DesktopPick(team));
                true
            }
            KeyCode::Char('p') => {
                self.offer_sign_in(None);
                true
            }
            _ => false,
        }
    }

    /// Text pasted (bracketed paste) where the keyboard is: a sign-in on the paste screen,
    /// else typed into the composer, the help filter or the command line.
    pub(super) fn paste(&mut self, text: String) {
        match self.key_context() {
            Ctx::Root if matches!(self.sign_in, Some(SignIn::Waiting | SignIn::Failed(_))) => {
                self.sign_in = Some(SignIn::Checking);
                self.effects.push(Effect::SignIn(Pasted(text)));
            }
            // Checking, or the desktop picker: one sign-in at a time, no paste into the picker.
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
