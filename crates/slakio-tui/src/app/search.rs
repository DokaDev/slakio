//! Searching a workspace's messages (`/`, `Space s`, `:search <words>`): a popup with the words
//! typed, in Slack's own search words (`in:#channel`, `from:@name`, quotes; passed on as typed),
//! and what they found, newest first, a page at a time. The search runs on `Enter`, or once
//! typing pauses ([`PAUSE`]); never per key, and never again for what is shown or on its way.
//! `Enter` on a result opens it as a Slack link to it opens ([`super::jump`]): the conversation
//! at the message, a reply's thread beside it. `Tab` searches the next workspace signed in to,
//! `PageDown` / `PageUp` the next or previous page. `Esc` closes it: what was asked and not
//! answered yet is dropped (an answer to it is never shown); the words and results stay for the
//! next time it opens.

use super::App;
use super::dialog::{Dialog, Question};
use super::jump::Offered;
use super::query::Focus;
use crate::action::SearchAction;
use crate::opener;
use crate::permalink::{self, Place, SlackLink};
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use slakio_core::backend::{Command, FailKind, Found, Generation, Hit};
use slakio_core::i18n::Msg;
use slakio_core::model::{Target, WorkspaceId};
use std::time::{Duration, Instant};

/// How long typing pauses before what was typed is searched.
pub const PAUSE: Duration = Duration::from_millis(700);

/// The most characters typed.
pub const TYPED_MAX: usize = 500;

/// The most runs (plain and found words) of a result's text kept for drawing.
pub const RUNS_MAX: usize = 64;

/// The most characters of a run kept for drawing (a long text before the first word found
/// keeps its end, the others their start): a line shows far fewer.
pub const RUN_CHARS_MAX: usize = 2_000;

/// The search popup's state.
#[derive(Clone, Debug, Default)]
pub(crate) struct Search {
    pub open: bool,
    pub query: String,
    /// The workspace searched.
    pub scope: Option<WorkspaceId>,
    /// The result under the cursor (of the page shown).
    pub selected: usize,
    /// The page shown, and the workspace it is of.
    pub found: Option<Found>,
    /// The search on its way.
    pub asked: Option<Asked>,
    /// When the words last changed, while they were not searched yet.
    pub typed: Option<Instant>,
    /// Why the last search failed, and its page (`Enter` asks for it again).
    pub failed: Option<FailKind>,
    pub retry: u32,
    /// Until when Slack asked to wait after the last failure, and the seconds that were.
    pub until: Option<(Instant, u64)>,
    /// `Enter` was pressed while Slack's wait was not over: the seconds it had left.
    pub left: Option<u64>,
    /// The runs of each result of the page shown ([`runs`]), worked out once.
    pub runs: Vec<Vec<(String, bool)>>,
    /// The words kept from before were not typed in since the popup opened: typing replaces
    /// them.
    pub fresh: bool,
}

/// A search on its way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Asked {
    pub id: Generation,
    pub query: String,
    pub page: u32,
    pub scope: WorkspaceId,
}

/// What the popup shows under the words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Showing {
    /// Nothing typed: how to search.
    Hint,
    /// Searching (nothing found yet for these words).
    Waiting,
    /// The last search failed.
    Failed,
    /// Nothing found.
    Nothing,
    /// Results.
    Results,
}

impl App {
    /// The search popup is open.
    pub fn searching(&self) -> bool {
        self.search.open
    }

    /// The words typed in the search popup.
    pub fn search_query(&self) -> &str {
        &self.search.query
    }

    /// The page of results shown, if any.
    pub fn search_found(&self) -> Option<&Found> {
        self.search.found.as_ref()
    }

    /// The result under the cursor (its index on the page shown).
    pub fn search_selected(&self) -> usize {
        self.search.selected
    }

    /// The workspace searched.
    pub fn search_scope(&self) -> Option<&WorkspaceId> {
        self.search.scope.as_ref()
    }

    /// A search is on its way.
    pub fn search_waiting(&self) -> bool {
        self.search.asked.is_some()
    }

    /// Why the last search failed, if it did.
    pub fn search_failed_kind(&self) -> Option<&FailKind> {
        self.search.failed.as_ref()
    }

    /// The seconds Slack asked to wait after the last failure, when it said; and, when `Enter`
    /// was pressed before they were over, the seconds that were left.
    pub fn search_wait(&self) -> (Option<u64>, Option<u64>) {
        (self.search.until.map(|(_, secs)| secs), self.search.left)
    }

    /// The runs of the result `i` of the page shown.
    pub fn search_runs(&self, i: usize) -> &[(String, bool)] {
        self.search.runs.get(i).map_or(&[], Vec::as_slice)
    }

    /// The words shown are kept from before, and typing replaces them.
    pub fn search_fresh(&self) -> bool {
        self.search.fresh
    }

    /// What the popup shows under the words.
    pub fn search_showing(&self) -> Showing {
        let s = &self.search;
        match &s.found {
            _ if s.failed.is_some() => Showing::Failed,
            Some(f) if !f.hits.is_empty() => Showing::Results,
            Some(_) if s.asked.is_none() => Showing::Nothing,
            _ if s.asked.is_some() => Showing::Waiting,
            _ => Showing::Hint,
        }
    }

    /// Open the search popup over the workspace the keyboard is in (the focused pane's, else the
    /// list's); with `query`, those words searched at once. Words kept from before that were not
    /// searched yet (closed while waiting) are searched once the pause passes; typing replaces
    /// them.
    pub(super) fn open_search(&mut self, query: Option<&str>, now: Instant) {
        if self.backend.is_none() || self.checking_sign_in() {
            return;
        }
        let here =
            self.work.focused().map(|p| p.target.workspace().clone()).or_else(|| self.shell.current_id(&self.model));
        let Some(here) = here else { return };
        (self.help, self.switcher) = (None, None);
        self.cmdline.close();
        if self.search.scope.as_ref() != Some(&here) {
            self.search = super::search::Search { query: std::mem::take(&mut self.search.query), ..Default::default() };
            self.search.scope = Some(here);
        }
        self.search.open = true;
        if let Some(q) = query {
            self.search.query = q.chars().filter(|c| !c.is_control()).take(TYPED_MAX).collect();
            (self.search.typed, self.search.fresh) = (None, false);
            self.ask_search(1);
            return;
        }
        self.search.fresh = !self.search.query.is_empty();
        if self.search_due() && self.search.failed.is_none() {
            self.search.typed = Some(now);
        }
    }

    /// A key typed in the search popup's input.
    pub(super) fn search_key(&mut self, code: KeyCode, mods: KeyModifiers, now: Instant) {
        match code {
            KeyCode::Char(c) if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                self.search_insert(c.encode_utf8(&mut [0; 4]), now);
            }
            KeyCode::Backspace if std::mem::take(&mut self.search.fresh) => {
                self.search.query.clear();
                self.search_typed(now);
            }
            KeyCode::Backspace if self.search.query.pop().is_some() => self.search_typed(now),
            _ => {}
        }
    }

    /// Type `text` in the search popup (a paste; control characters left out). Words kept from
    /// before are replaced.
    pub(super) fn search_insert(&mut self, text: &str, now: Instant) {
        if std::mem::take(&mut self.search.fresh) {
            self.search.query.clear();
        }
        let room = TYPED_MAX.saturating_sub(self.search.query.chars().count());
        self.search.query.extend(text.chars().filter(|c| !c.is_control()).take(room));
        self.search_typed(now);
    }

    fn search_typed(&mut self, now: Instant) {
        self.search.typed = Some(now);
        (self.search.failed, self.search.until, self.search.left) = (None, None, None);
    }

    pub(super) fn search_action(&mut self, a: SearchAction, now: Instant) {
        if !self.search.open {
            return;
        }
        let hits = self.search.found.as_ref().map_or(0, |f| f.hits.len());
        match a {
            // Slack's wait is not over: nothing is sent, and the popup says how long is left.
            SearchAction::Run if self.search.until.is_some_and(|(until, _)| now < until) => {
                let left = self.search.until.map(|(until, _)| until.saturating_duration_since(now));
                self.search.left = left.map(|d| d.as_secs() + u64::from(d.subsec_nanos() > 0));
            }
            SearchAction::Run if self.search_due() => self.ask_search(1),
            SearchAction::Run if self.search.failed.is_some() => self.ask_search(self.search.retry.max(1)),
            SearchAction::Run => self.open_hit(now),
            SearchAction::Next => self.search.selected = (self.search.selected + 1).min(hits.saturating_sub(1)),
            SearchAction::Prev => self.search.selected = self.search.selected.saturating_sub(1),
            SearchAction::PageNext | SearchAction::PagePrev => {
                let Some(f) = &self.search.found else { return };
                // Only the words shown are paged: new words on their way (or typed) are not
                // paged with the page count of the old ones.
                let asked_other =
                    self.search.asked.as_ref().is_some_and(|a| a.query != f.query || a.scope != f.workspace);
                if f.query != self.search.query.trim()
                    || self.search.scope.as_ref() != Some(&f.workspace)
                    || asked_other
                {
                    return;
                }
                let page = if a == SearchAction::PageNext { f.page + 1 } else { f.page.saturating_sub(1) };
                if (1..=f.pages).contains(&page) && !self.search_due() {
                    self.ask_search(page);
                }
            }
            SearchAction::ScopeNext | SearchAction::ScopePrev => {
                let all: Vec<WorkspaceId> = self.model.workspaces().iter().map(|w| w.id.clone()).collect();
                let at = self.search.scope.as_ref().and_then(|s| all.iter().position(|w| w == s)).unwrap_or(0);
                let by = if a == SearchAction::ScopeNext { 1 } else { all.len().saturating_sub(1) };
                let Some(next) = all.get((at + by) % all.len().max(1)).cloned() else { return };
                if Some(&next) != self.search.scope.as_ref() {
                    (self.search.scope, self.search.found, self.search.asked) = (Some(next), None, None);
                    self.search.failed = None;
                    self.ask_search(1);
                }
            }
            SearchAction::Clear => {
                (self.search.fresh, self.search.until, self.search.left) = (false, None, None);
                self.search.query.clear();
                (self.search.found, self.search.asked, self.search.typed, self.search.failed) =
                    (None, None, None, None);
            }
            SearchAction::Close => self.close_search(),
        }
    }

    /// Close the popup: what is on its way is no longer waited for.
    pub(super) fn close_search(&mut self) {
        self.search.open = false;
        (self.search.asked, self.search.typed) = (None, None);
    }

    /// The words typed are not what is shown or on its way: `Enter` searches them.
    fn search_due(&self) -> bool {
        let s = &self.search;
        let q = s.query.trim();
        let same = |query: &str, scope: &WorkspaceId| query == q && s.scope.as_ref() == Some(scope);
        let shown = s.found.as_ref().is_some_and(|f| same(&f.query, &f.workspace));
        let asked = s.asked.as_ref().is_some_and(|a| same(&a.query, &a.scope));
        !q.is_empty() && !shown && !asked
    }

    /// Search page `page` of the words typed, unless that very page is shown or on its way.
    fn ask_search(&mut self, page: u32) {
        self.search.typed = None;
        let query = self.search.query.trim().to_string();
        let Some(scope) = self.search.scope.clone() else { return };
        if query.is_empty() {
            (self.search.found, self.search.asked) = (None, None);
            return;
        }
        let same = |q: &str, p: u32, w: &WorkspaceId| q == query && p == page && *w == scope;
        if self.search.asked.as_ref().is_some_and(|a| same(&a.query, a.page, &a.scope))
            || self.search.found.as_ref().is_some_and(|f| same(&f.query, f.page, &f.workspace))
        {
            return;
        }
        let id = self.work.requests.ask(Some(scope.clone()), Command::Search { query: query.clone(), page });
        self.search.asked = Some(Asked { id, query, page, scope });
        (self.search.failed, self.search.until, self.search.left) = (None, None, None);
    }

    /// Typing paused long enough at `now`: what was typed is searched. `true` when it was.
    pub(super) fn search_tick(&mut self, now: Instant) -> bool {
        match self.search.typed {
            Some(at) if self.search.open && now >= at + PAUSE => {
                self.search.typed = None;
                if self.search_due() {
                    self.ask_search(1);
                } else if self.search.query.trim().is_empty() {
                    (self.search.found, self.search.asked) = (None, None);
                }
                true
            }
            _ => false,
        }
    }

    /// When typing will have paused long enough.
    pub(super) fn search_deadline(&self) -> Option<Instant> {
        self.search.typed.filter(|_| self.search.open).map(|at| at + PAUSE)
    }

    /// A page of results came for request `id`: shown when it is the search waited for.
    pub(super) fn on_found(&mut self, id: Generation, mut found: Found) -> bool {
        let Some(asked) = self.search.asked.take_if(|a| a.id == id) else { return false };
        self.model.add_users(&asked.scope, &found.users);
        (found.workspace, found.query, found.page) = (asked.scope, asked.query, asked.page);
        found.pages = found.pages.max(u32::from(!found.hits.is_empty()));
        self.search.runs = found.hits.iter().map(runs).collect();
        (self.search.found, self.search.selected, self.search.failed) = (Some(found), 0, None);
        true
    }

    /// Request `id` failed at `now`: when it is the search waited for, the popup says why (and,
    /// when Slack said, how long to wait); its workspace is returned.
    pub(super) fn search_failed(&mut self, id: Generation, kind: &FailKind, now: Instant) -> Option<WorkspaceId> {
        let asked = self.search.asked.take_if(|a| a.id == id)?;
        (self.search.failed, self.search.retry, self.search.left) = (Some(kind.clone()), asked.page, None);
        self.search.until = match kind {
            FailKind::RateLimited { retry_after: Some(d) } if !d.is_zero() => {
                Some((now + *d, d.as_secs() + u64::from(d.subsec_nanos() > 0)))
            }
            _ => None,
        };
        Some(asked.scope)
    }

    /// The result under the cursor opened where its conversation is: as a Slack link to it opens,
    /// the popup closed. A conversation the workspace does not list is offered for the browser.
    fn open_hit(&mut self, now: Instant) {
        let (Some(f), Some(ws)) = (&self.search.found, self.search.scope.clone()) else { return };
        let Some(hit) = f.hits.get(self.search.selected).cloned() else { return };
        self.close_search();
        let listed = self.model.conversations().iter().any(|c| c.workspace == ws && c.id == hit.conversation);
        let web = self.hit_link(&ws, &hit);
        if !listed {
            let why = Msg::JumpUnlisted { host: self.workspace_label(&ws) };
            match web {
                Some(url) => {
                    self.jumps.offered = Some(Offered { url, label: None, why });
                    self.dialog = Some(Dialog::new(Question::Browser));
                }
                None => self.info(why, now),
            }
            return;
        }
        let target = Target::Conversation { workspace: ws.clone(), conversation: hit.conversation.clone() };
        let link = SlackLink {
            place: Place::Team(ws.as_str().to_string()),
            conversation: hit.conversation.as_str().to_string(),
            message: Some(hit.ts),
            thread: hit.thread,
        };
        self.jump_to(target, &link, web.as_deref().unwrap_or_default(), None, now);
        if web.is_none() {
            self.jumps.asked = None;
        }
        if let Some(id) = self.work.active() {
            self.set_focus(Focus::on(id));
        }
    }

    /// `hit`'s web link, offered for the browser only when it is a Slack link to the message's
    /// conversation in the workspace searched: on that workspace's host (or its organization's
    /// Enterprise Grid host, or its id), naming the hit's conversation.
    fn hit_link(&self, ws: &WorkspaceId, hit: &Hit) -> Option<String> {
        let link = permalink::parse(&hit.permalink)?;
        let ours = match &link.place {
            Place::Host(h) => self.jumps.hosts.iter().any(|(w, x)| w == ws && x == h),
            Place::Enterprise(_) => true,
            Place::Team(t) => t == ws.as_str(),
        };
        (ours && link.conversation == hit.conversation.as_str())
            .then(|| opener::check(&hit.permalink))
            .flatten()
            .map(|c| c.url)
    }

    /// How a workspace is named in a question: its host, else its name, else its id.
    pub fn workspace_label(&self, ws: &WorkspaceId) -> String {
        let host = self.jumps.hosts.iter().find(|(w, h)| w == ws && !h.is_empty()).map(|(_, h)| h.clone());
        host.or_else(|| self.model.workspace(ws).map(|w| w.name.line().into_string()).filter(|n| !n.is_empty()))
            .unwrap_or_else(|| ws.as_str().to_string())
    }
}

/// `hit`'s text as runs of plain and matched words, the marks left out (not sanitised yet): at
/// most [`RUNS_MAX`] runs, each at most [`RUN_CHARS_MAX`] characters (the first, when plain,
/// keeps its end: the words found follow it).
pub fn runs(hit: &Hit) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut matched = false;
    let mut run = String::new();
    for c in hit.text.unsanitized().chars() {
        if out.len() >= RUNS_MAX {
            break;
        }
        let mark = match c {
            slakio_core::backend::MATCH_START => Some(true),
            slakio_core::backend::MATCH_END => Some(false),
            _ => None,
        };
        match mark {
            Some(m) => {
                if !run.is_empty() {
                    out.push((std::mem::take(&mut run), matched));
                }
                matched = m;
            }
            None => run.push(c),
        }
    }
    if !run.is_empty() && out.len() < RUNS_MAX {
        out.push((run, matched));
    }
    for (k, (run, matched)) in out.iter_mut().enumerate() {
        let n = run.chars().count();
        if n > RUN_CHARS_MAX {
            *run = if k == 0 && !*matched {
                run.chars().skip(n - RUN_CHARS_MAX).collect()
            } else {
                run.chars().take(RUN_CHARS_MAX).collect()
            };
        }
    }
    out
}

mod view;
pub use view::{HIT_LINES, HITS_MAX, SearchBox, search_box};

#[cfg(test)]
mod tests;
