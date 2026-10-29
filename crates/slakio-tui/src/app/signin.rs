//! Signing in from the app. `slakio login` (and the first run without a workspace) first offers
//! the workspaces the Slack desktop app is signed in to, as a picker: `Space` marks several,
//! `Enter` signs in to the marked ones (else the one under the cursor); they go to the binary by
//! team id ([`Effect::DesktopPick`]), which reads its credentials (the macOS keychain
//! may prompt), asks Slack whether they sign in and keeps them. Pressing `p` (or no desktop app) falls back to
//! the paste flow: the screen says how to copy a request, what is pasted there goes to the binary
//! ([`Effect::SignIn`]). The pasted text is never kept by the app, drawn or printed.
//!
//! A workspace Slack signs out while the app runs (any answer refused for the sign-in, or its
//! socket) stays on screen as it was, marked `signed out` until it is signed in again: `:login`
//! offers the same screen over the app ([`Effect::SignInAgain`], the picker's cursor on that
//! workspace; `Esc` goes back), and once signed in again only that workspace starts over —
//! booted again, its open panes caught up and their failed pages asked again; tabs, panes, where
//! they were read and what is being written stay.

use super::model::SignedIn;
use super::status::Level;
use super::{App, Effect};
use crate::action::SignInAction;
use crate::keymap::Ctx;
use slakio_core::backend::{Capabilities, Command, FailKind};
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::{Workspace, WorkspaceId};
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

/// Signing in as the app offers it: where it stands (`None` when it is not offered), and the
/// workspaces signed in to already, which stay (what is signed in to is added beside them).
#[derive(Clone, Debug, Default)]
pub(crate) struct Offer {
    pub state: Option<SignIn>,
    pub beside: Vec<String>,
}

/// One workspace of the desktop picker: its team id, its name and its host (Slack's text,
/// already sanitised). The host tells apart two workspaces of the same name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerRow {
    pub team: String,
    pub name: String,
    pub host: String,
}

/// The desktop-workspace picker: its rows, the one under the cursor, the ones marked (by team
/// id) to sign in to together, and the reason the last pick did not sign in, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picker {
    pub rows: Vec<PickerRow>,
    pub selected: usize,
    pub marked: Vec<String>,
    pub note: Option<Label>,
}

impl Picker {
    /// Row `i` is marked.
    pub fn is_marked(&self, i: usize) -> bool {
        self.rows.get(i).is_some_and(|r| self.marked.contains(&r.team))
    }

    /// What `Enter` signs in to: the marked workspaces in the picker's order, else the one under
    /// the cursor.
    pub fn chosen(&self) -> Vec<String> {
        let marked: Vec<String> =
            self.rows.iter().filter(|r| self.marked.contains(&r.team)).map(|r| r.team.clone()).collect();
        if marked.is_empty() {
            self.rows.get(self.selected).map(|r| r.team.clone()).into_iter().collect()
        } else {
            marked
        }
    }
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
        self.sign_in.state = Some(why.map_or(SignIn::Waiting, SignIn::Failed));
    }

    /// Offer the desktop app's signed-in workspaces as a picker. The pick is reported by team id.
    pub fn offer_desktop(&mut self, rows: Vec<PickerRow>) {
        self.sign_in.state = Some(SignIn::Desktop(Picker { rows, selected: 0, marked: Vec::new(), note: None }));
    }

    /// Signing in adds a workspace beside `workspaces` (signed in to already, Slack's names,
    /// already sanitised), which stay signed in: the screen says so.
    pub fn sign_in_beside(&mut self, workspaces: Vec<String>) {
        self.sign_in.beside = workspaces;
    }

    /// The workspaces signed in to already that signing in adds one beside (none at first).
    pub fn signed_in_already(&self) -> &[String] {
        &self.sign_in.beside
    }

    /// Where signing in stands; `None` when the app does not offer it.
    pub fn sign_in(&self) -> Option<SignIn> {
        self.sign_in.state.clone()
    }

    /// The paste being checked did not sign in, for `why`.
    pub fn sign_in_failed(&mut self, why: Label) {
        match &self.sign_in.state {
            // A desktop pick that failed returns to the picker with the reason shown.
            Some(SignIn::DesktopChecking(p)) => {
                let mut p = p.clone();
                p.note = Some(why);
                self.sign_in.state = Some(SignIn::Desktop(p));
            }
            Some(_) => self.sign_in.state = Some(SignIn::Failed(why)),
            None => {}
        }
    }

    /// Signed in to `workspaces` (Slack's names, already sanitised), the ones of `signed` that
    /// are new, the first of them `added`: the app connects to the backend of every workspace of
    /// `signed`, which can do `caps`, and shows the one just added.
    pub fn signed_in(
        &mut self,
        caps: Capabilities,
        signed: &[SignedIn],
        added: Option<&WorkspaceId>,
        workspaces: &[String],
        now: Instant,
    ) {
        self.sign_in = Offer::default();
        self.connect_workspaces(caps, signed);
        if let Some(at) = added.and_then(|ws| signed.iter().position(|s| &s.id == ws)) {
            self.shell.select_workspace(at, &self.model);
        }
        self.info(Msg::SigninSignedIn { workspace: workspaces.join(", ") }, now);
    }

    /// The desktop picker has the keyboard (its keys are [`Ctx::SignIn`]'s).
    pub(super) fn picking(&self) -> bool {
        matches!(self.sign_in.state, Some(SignIn::Desktop(_)))
    }

    /// An action of the desktop picker: move, pick (the binary reads and checks that workspace),
    /// or switch to the paste flow.
    pub(super) fn sign_in_action(&mut self, a: SignInAction) {
        if a == SignInAction::Cancel {
            return self.sign_in_cancel();
        }
        let Some(SignIn::Desktop(p)) = &mut self.sign_in.state else { return };
        match a {
            SignInAction::Down => p.selected = (p.selected + 1).min(p.rows.len().saturating_sub(1)),
            SignInAction::Up => p.selected = p.selected.saturating_sub(1),
            SignInAction::Mark => {
                let Some(team) = p.rows.get(p.selected).map(|r| r.team.clone()) else { return };
                match p.marked.iter().position(|t| *t == team) {
                    Some(at) => drop(p.marked.remove(at)),
                    None => p.marked.push(team),
                }
            }
            SignInAction::Pick => {
                let teams = p.chosen();
                if teams.is_empty() {
                    return;
                }
                let p = Picker { note: None, ..p.clone() };
                self.sign_in.state = Some(SignIn::DesktopChecking(p));
                self.effects.push(Effect::DesktopPick(teams));
            }
            SignInAction::Paste => self.offer_sign_in(None),
            SignInAction::Cancel => {}
        }
    }

    /// `Esc` on the sign-in screen over the app: back to it, signed in as before (not while a
    /// sign-in is being checked; the first sign-in has nothing to go back to).
    fn sign_in_cancel(&mut self) {
        let checking = matches!(self.sign_in.state, Some(SignIn::Checking | SignIn::DesktopChecking(_)));
        if self.backend.is_some() && !checking {
            self.sign_in = Offer::default();
        }
    }

    /// The desktop picker's cursor on the workspace of team id `team`, when it lists it.
    pub fn sign_in_select(&mut self, team: &str) {
        if let Some(SignIn::Desktop(p)) = &mut self.sign_in.state
            && let Some(at) = p.rows.iter().position(|r| r.team == team)
        {
            p.selected = at;
        }
    }

    /// Slack no longer accepts the sign-in of `ws`, which was loaded: it stays on screen as it
    /// was, nothing more is asked of it (its backend sends nothing more), and the status line
    /// says so and what to do, then keeps a mark of it.
    pub(super) fn signed_out(&mut self, ws: WorkspaceId, now: Instant) {
        let several = self.boots.len() > 1;
        let Some(b) = self.boots.iter_mut().find(|b| b.workspace.as_ref().is_none_or(|w| *w == ws)) else { return };
        if std::mem::replace(&mut b.signed_out, true) {
            return;
        }
        self.forget_live(&ws, now);
        match self.model.workspace(&ws).filter(|_| several).cloned() {
            Some(w) => {
                let why = self.i18n.label(Label::SignedOutWhy).to_string();
                let msg = Msg::FailWorkspace { workspace: w.name.line().into_string(), why };
                self.status.show_about(msg, Level::Warning, Some(w.id.clone()), now);
            }
            None => self.warn(Msg::Label(Label::SignedOut), now),
        }
    }

    /// Workspace `ws` was signed out while the app ran (and not signed in again since).
    pub fn signed_out_in(&self, ws: &WorkspaceId) -> bool {
        self.boots.iter().find(|b| b.workspace.as_ref().is_none_or(|w| w == ws)).is_some_and(|b| b.signed_out)
    }

    /// The workspaces signed out while the app ran, in the switcher's order.
    pub fn signed_out_workspaces(&self) -> Vec<&Workspace> {
        self.model.workspaces().iter().filter(|w| self.signed_out_in(&w.id)).collect()
    }

    /// `:login`: sign in again, the picker's cursor on the workspace shown when Slack does not
    /// accept its sign-in, else on the first that it does not accept (else on none).
    pub(super) fn sign_in_again(&mut self) {
        if !self.networked() {
            return;
        }
        let refused = |ws: &WorkspaceId| self.signed_out_in(ws) || self.boot_failed_in(ws) == Some(&FailKind::Auth);
        let shown = self.shell.current_id(&self.model).filter(|ws| refused(ws));
        let team = shown.or_else(|| self.model.workspaces().iter().map(|w| w.id.clone()).find(|ws| refused(ws)));
        self.effects.push(Effect::SignInAgain(team.map(|t| t.as_str().to_string())));
    }

    /// Signed in again to the workspaces `signed` (Slack's names `workspaces`), which the app
    /// shows already: the binary started their backends again with the new sign-in, the others
    /// go on as they were. Each boots again (its part of the model replaced when it answers),
    /// then its open panes catch up; the pages that failed are asked for again. Tabs, panes,
    /// where they were read and drafts stay.
    pub fn signed_in_again(&mut self, signed: &[SignedIn], workspaces: &[String], now: Instant) {
        self.sign_in = Offer::default();
        for s in signed {
            let id = self.work.requests.ask(Some(s.id.clone()), Command::Boot);
            if let Some(b) = self.boots.iter_mut().find(|b| b.workspace.as_ref() == Some(&s.id)) {
                (b.id, b.failed, b.signed_out, b.again) = (id, None, false, true);
                b.read_only |= s.read_only;
            }
            self.work.retry_signed_in(&s.id);
        }
        // What was said of the sign-out is no longer true.
        let why = self.i18n.label(Label::SignedOutWhy).to_string();
        self.status.forget(now, |m| match m {
            Msg::Label(l) => *l == Label::SignedOut,
            Msg::FailWorkspace { why: w, .. } => *w == why,
            _ => false,
        });
        self.info(Msg::SigninSignedIn { workspace: workspaces.join(", ") }, now);
    }

    /// Text pasted (bracketed paste) where the keyboard is: a sign-in on the paste screen or the
    /// desktop picker (a request copied first is taken there too), else typed into the composer,
    /// the help filter or the command line.
    pub(super) fn paste(&mut self, text: String) {
        match self.key_context() {
            // Checking (over the app, the keyboard is the sign-in screen's then too): one at a time.
            Ctx::SignIn if matches!(self.sign_in.state, Some(SignIn::Checking | SignIn::DesktopChecking(_))) => {}
            Ctx::SignIn => {
                self.sign_in.state = Some(SignIn::Checking);
                self.effects.push(Effect::SignIn(Pasted(text)));
            }
            Ctx::Root if matches!(self.sign_in.state, Some(SignIn::Waiting | SignIn::Failed(_))) => {
                self.sign_in.state = Some(SignIn::Checking);
                self.effects.push(Effect::SignIn(Pasted(text)));
            }
            // Checking: one sign-in at a time.
            Ctx::Root if self.sign_in.state.is_some() => {}
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
