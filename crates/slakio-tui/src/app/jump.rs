//! Slack's own links opened inside slakio (`o`, a click on one, a pick, `:open <address>`):
//! a link to a workspace signed in to, to a conversation it lists, opens there
//! ([`super::work::jump`]). One whose workspace is not signed in to, whose conversation the
//! workspace does not list, or that Slack refuses to read, is offered for the browser instead —
//! a question, the safe answer focused; yes opens it as any web link opens (its own questions
//! included).
//!
//! The workspace is the one whose host the link names, or whose id (the web client's links),
//! and only that one: a link naming a workspace that does not list the conversation never opens
//! it in another. Only a link to an Enterprise Grid organization (its host, or its `E…` id), which
//! names no single workspace, opens in any workspace signed in to that lists the conversation.

use super::App;
use super::dialog::{Dialog, Question};
use super::query::Focus;
use crate::permalink::{self, Place, SlackLink};
use slakio_core::backend::{FailKind, Generation};
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::{ConversationId, Target, WorkspaceId};
use std::time::Instant;

/// What the app keeps of Slack links.
#[derive(Clone, Debug, Default)]
pub(crate) struct Jumps {
    /// Each workspace signed in to and its host, lower case.
    pub hosts: Vec<(WorkspaceId, String)>,
    /// The link offered for the browser, while asked.
    pub offered: Option<Offered>,
    /// The request reading around the message the last link opened points at, and that link.
    pub asked: Option<Asked>,
}

/// A link whose message is being read around, while it is.
#[derive(Clone, Debug)]
pub(crate) struct Asked {
    pub id: Generation,
    pub target: Target,
    pub url: String,
    pub label: Option<String>,
}

/// Slack's error codes that mean the conversation cannot be read here (but may be in the
/// browser): only these offer it there.
pub const NOT_HERE: [&str; 5] =
    ["channel_not_found", "not_in_channel", "access_denied", "missing_scope", "is_archived"];

/// A Slack link offered for the browser, and why it does not open here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offered {
    pub url: String,
    pub label: Option<String>,
    pub why: Msg,
}

/// Where a link's conversation is.
enum Found {
    Here(Target),
    /// Its workspace is signed in to, but does not list it.
    Unlisted,
    /// No workspace signed in to is its.
    Elsewhere,
}

impl App {
    /// Open `url` here when it is a Slack link (whose text is `label`), or offer it for the
    /// browser; `false` when it is not one (or there is no backend to read it).
    pub(super) fn open_slack(&mut self, url: &str, label: Option<&str>, now: Instant) -> bool {
        let Some(link) = permalink::parse(url).filter(|_| self.backend.is_some()) else { return false };
        let host = match &link.place {
            Place::Host(h) | Place::Enterprise(h) => h.clone(),
            // A workspace signed in to is named by its host (or name), not its id.
            Place::Team(t) => match self.model.workspaces().iter().find(|w| w.id.as_str() == t) {
                Some(w) => self.workspace_label(&w.id.clone()),
                None => t.clone(),
            },
        };
        match self.find(&link) {
            Found::Here(target) => self.jump_to(target, &link, url, label, now),
            Found::Unlisted => self.offer_browser(url, label, Msg::JumpUnlisted { host }),
            Found::Elsewhere => self.offer_browser(url, label, Msg::JumpElsewhere { host }),
        }
        true
    }

    fn find(&self, link: &SlackLink) -> Found {
        let id = ConversationId::new(&link.conversation);
        let named: Vec<&WorkspaceId> = match &link.place {
            Place::Host(h) => self.jumps.hosts.iter().filter(|(_, x)| x == h).map(|(w, _)| w).collect(),
            Place::Team(t) if !t.starts_with('E') => {
                self.model.workspaces().iter().map(|w| &w.id).filter(|w| w.as_str() == t).collect()
            }
            // An Enterprise Grid organization names none of its workspaces: one signed in to that
            // lists the conversation.
            Place::Enterprise(_) | Place::Team(_) => {
                let c = self.model.conversations().iter().find(|c| c.id == id);
                return c.map_or(Found::Elsewhere, |c| {
                    Found::Here(Target::Conversation { workspace: c.workspace.clone(), conversation: id.clone() })
                });
            }
        };
        let lists = |ws: &WorkspaceId| self.model.conversations().iter().any(|c| c.workspace == *ws && c.id == id);
        match named.iter().copied().find(|ws| lists(ws)).cloned() {
            Some(workspace) => Found::Here(Target::Conversation { workspace, conversation: id }),
            None if named.is_empty() => Found::Elsewhere,
            None => Found::Unlisted,
        }
    }

    /// Open `conversation` where `link` points: the conversation, its message, or a reply in
    /// its thread beside it; the pane opened gets the keyboard.
    pub(super) fn jump_to(
        &mut self,
        conversation: Target,
        link: &SlackLink,
        url: &str,
        label: Option<&str>,
        now: Instant,
    ) {
        let target = conversation.clone();
        let (pane, asked) = match (link.message, link.thread) {
            (None, _) => (self.work.open(conversation), None),
            (Some(at), None) => self.work.jump(conversation, at),
            (Some(reply), Some(parent)) => self.work.jump_reply(conversation, parent, reply),
        };
        let (url, label) = (url.to_string(), label.map(str::to_string));
        self.jumps.asked = asked.map(|id| Asked { id, target, url, label });
        if let Some(id) = pane {
            self.work.activate(id);
            self.set_focus(Focus::on(id));
        }
        self.landed(now);
    }

    fn offer_browser(&mut self, url: &str, label: Option<&str>, why: Msg) {
        self.jumps.offered = Some(Offered { url: url.to_string(), label: label.map(str::to_string), why });
        self.dialog = Some(Dialog::new(Question::Browser));
    }

    /// The answer to "open in the browser?": yes opens it as a web link.
    pub(super) fn answer_browser(&mut self, yes: bool, now: Instant) {
        if let Some(o) = self.jumps.offered.take().filter(|_| yes) {
            self.open_web(&o.url, o.label.as_deref(), now);
        }
    }

    /// The link offered for the browser, while asked.
    pub fn offered(&self) -> Option<&Offered> {
        self.jumps.offered.as_ref()
    }

    /// Request `id` was answered: when it read around a link's message, nothing more waits.
    /// When Slack refused to read it (its own error: not in the channel, no such channel), the
    /// link is offered for the browser.
    pub(super) fn jump_answered(&mut self, id: Generation, failed: Option<&FailKind>) {
        let Some(a) = self.jumps.asked.take_if(|a| a.id == id) else { return };
        let not_here = matches!(failed, Some(FailKind::Api { code }) if NOT_HERE.contains(&code.as_str()));
        if not_here && self.dialog.is_none() {
            self.offer_browser(&a.url, a.label.as_deref(), Msg::Label(Label::JumpRefused));
        }
    }

    /// The link whose message is read around waits no more when nothing shows its conversation
    /// any more, or when `newest` (`G` there): its answer, or its failure, is no longer asked for.
    pub(super) fn forget_asked(&mut self, newest: bool) {
        let shown = |t: &Target| self.work.showing(t).is_some();
        let focused = self.work.focused().map(|p| p.target.clone());
        if self
            .jumps
            .asked
            .as_ref()
            .is_some_and(|a| !shown(&a.target) || (newest && focused.as_ref() == Some(&a.target)))
        {
            self.jumps.asked = None;
        }
    }

    /// A link's message just selected is marked from `now`, whatever selected it (its page, or
    /// a tab shown again); one that is not there any more is said instead.
    pub(super) fn landed(&mut self, now: Instant) {
        self.work.arm_flash(now);
        if self.work.take_missed() {
            self.info(Msg::Label(Label::JumpGone), now);
        }
        if let Some(at) = self.work.take_quiet() {
            let day = crate::time::date(at, &self.zone);
            self.info(Msg::JumpDateQuiet { day }, now);
        }
    }

    /// The hosts of the workspaces `signed` signed in to: a Slack link to one opens there.
    pub(super) fn set_hosts(&mut self, signed: &[super::model::SignedIn]) {
        self.jumps.hosts = signed.iter().map(|s| (s.id.clone(), s.host.to_ascii_lowercase())).collect();
    }
}
