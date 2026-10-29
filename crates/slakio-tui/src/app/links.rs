//! What the selected message holds, taken out of the app: its text copied (`y`), its links
//! opened in the browser (`o`, `:open`, `:open 2` for its second, `:open <address>`). Opening is
//! a local action — nothing goes to Slack — so a read-only workspace opens links too. Only a web
//! address opens ([`opener::check`]); one whose text names another host than it goes to is
//! asked about first, the real host shown.

use super::dialog::{Dialog, Question};
use super::{App, Effect};
use crate::mrkdwn::{self, Block, Piece};
use crate::opener;
use crate::text::clip;
use slakio_core::i18n::{Label, Msg};
use std::time::Instant;

/// A link waiting for the user's yes: its text names another host than the one it goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opening {
    pub url: String,
    /// Where it goes.
    pub host: String,
    /// What its text says (cut).
    pub said: String,
}

/// The most cells of a link's text the question shows.
const SAID_MAX: usize = 40;

/// The most of a host the status line names (its end), so the words fit at 80 columns.
const HOST_SAID: usize = 19;

impl App {
    /// Copy the selected messages (the VISUAL range, or the selected one) as text: one
    /// message alone as its text, a range as `time  author: text` lines. Sanitised text only.
    pub(super) fn copy(&mut self, now: Instant) {
        let Some(p) = self.work.focused() else { return };
        let Some((a, b)) = p.range(self.work.timeline(p)) else { return };
        let one = a == b && p.visual.is_none();
        let items = &self.work.timeline(p).items[a..=b];
        let text = if one {
            items[0].text.to_string()
        } else {
            items
                .iter()
                .map(|m| format!("{}  {}: {}", crate::time::hm(m.ts), m.author, m.text))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let count = items.len() as u64;
        if let Some(p) = self.work.focused_mut() {
            p.visual = None;
        }
        self.effects.push(Effect::Copy(text));
        self.info(Msg::StatusCopied { count }, now);
    }

    /// The links of the selected message (of the focused pane, else the active one), in their
    /// order: each one's address and its text, if any.
    fn selected_links(&self) -> Vec<(String, Option<String>)> {
        let pane = self.work.focused().or_else(|| self.work.active().and_then(|id| self.work.pane(id)));
        let Some(p) = pane else { return Vec::new() };
        let tl = self.work.timeline(p);
        let Some((a, _)) = p.range(tl) else { return Vec::new() };
        let blocks = mrkdwn::parse(&tl.items[a].text.to_string());
        let pieces = blocks.into_iter().flat_map(|b| match b {
            Block::Line { pieces, .. } => pieces,
            Block::Code(_) => Vec::new(),
        });
        pieces.filter_map(|p| if let Piece::Link { url, label, .. } = p { Some((url, label)) } else { None }).collect()
    }

    /// `o` (`:open`): open link `n` (from 1) of the selected message.
    pub(super) fn open_link(&mut self, n: usize, now: Instant) {
        match n.checked_sub(1).and_then(|i| self.selected_links().into_iter().nth(i)) {
            Some((url, label)) => self.open_address(&url, label.as_deref(), now),
            None => self.info(Msg::Label(Label::OpenNone), now),
        }
    }

    /// `:open <arg>`: the selected message's link `arg` when it is a number, else the address
    /// typed.
    pub(super) fn open_typed(&mut self, arg: &str, now: Instant) {
        match arg.parse::<usize>() {
            Ok(n) => self.open_link(n, now),
            Err(_) => self.open_address(arg, None, now),
        }
    }

    /// Open `url`, a link's address whose text is `label`: refused unless it is a web address
    /// ([`opener::check`], handed over in its checked form); asked about first when the text
    /// names another host; else handed to the binary, the host it goes to said.
    fn open_address(&mut self, url: &str, label: Option<&str>, now: Instant) {
        let Some(checked) = opener::check(url) else { return self.warn(Msg::Label(Label::OpenRefused), now) };
        match label.filter(|l| mrkdwn::real_target(url, l).is_some()) {
            Some(said) => {
                let said = clip(said, SAID_MAX);
                self.opening = Some(Opening { url: checked.url, host: checked.host, said });
                self.dialog = Some(Dialog::new(Question::Open));
            }
            None => {
                self.effects.push(Effect::OpenLink(checked.url));
                // The end of a long host is what says whose it is: cut from its start, last.
                self.info(Msg::OpenOpening { host: opener::host_tail(&checked.host, HOST_SAID) }, now);
            }
        }
    }

    /// The answer to "open this link?": yes hands it to the binary.
    pub(super) fn answer_open(&mut self, yes: bool) {
        if let Some(o) = self.opening.take().filter(|_| yes) {
            self.effects.push(Effect::OpenLink(o.url));
        }
    }

    /// The palette's row of `:open <arg>`.
    pub(super) fn open_row(&self, arg: &str) -> super::palette::Row {
        let label = self.i18n.label(Label::ActionPaneOpenLink).to_string();
        super::palette::Row { name: format!(":open {arg}"), label, keys: String::new() }
    }

    /// The link asked about, while asked.
    pub fn opening(&self) -> Option<&Opening> {
        self.opening.as_ref()
    }
}
