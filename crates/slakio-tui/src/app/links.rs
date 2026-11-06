//! What the selected message holds, taken out of the app: its text copied (`y`), its links
//! opened in the browser (`o`, `:open`, a click on one, `:open <address>` typed). With several
//! links that open, `o` picks one ([`super::picker`]). Opening is a local action — nothing goes to Slack — so a read-only
//! workspace opens links too. Only a web address opens ([`opener::check`]): any other link gets
//! no letters, and a click on one says why. One whose text names another host than it goes to
//! is asked about first, the real host shown.

use super::dialog::{Dialog, Question};
use super::pane::LinkSpot;
use super::status::NOTICE_MAX;
use super::{App, Effect};
use crate::mrkdwn::{self, Block, Piece};
use crate::opener;
use crate::text::clip;
use slakio_core::i18n::{Label, Msg};
use slakio_core::layout::PaneId;
use slakio_core::model::Ts;
use std::time::Instant;

/// The links of a message's text (code blocks left out), in their order: each one's address
/// and its text, if any. Drawing numbers them the same way.
pub fn links_of(body: &[Block]) -> Vec<(String, Option<String>)> {
    let pieces = body.iter().flat_map(|b| match b {
        Block::Line { pieces, .. } => pieces.as_slice(),
        Block::Code(_) => &[],
    });
    pieces
        .filter_map(|p| if let Piece::Link { url, label, .. } = p { Some((url.clone(), label.clone())) } else { None })
        .collect()
}

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

/// From this width on, the status line has the room it makes for a notice ([`NOTICE_MAX`]).
const WIDE: u16 = 120;

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

    /// `o` (`:open`): the selected message's link, when one opens; when several do, they are
    /// picked ([`super::picker`]). A message with no link says so; one whose links all are
    /// refused says why.
    pub(super) fn open_link(&mut self, now: Instant) {
        let Some((pane, ts, body)) = self.selected_body() else { return self.info(Msg::Label(Label::OpenNone), now) };
        let links = links_of(&body);
        let open = super::picker::openable(&body);
        match open[..] {
            _ if links.is_empty() => self.info(Msg::Label(Label::OpenNone), now),
            [] => self.warn(Msg::Label(Label::OpenRefused), now),
            [(i, _)] => self.open_address(&links[i].0, links[i].1.as_deref(), now),
            _ => self.start_picking((pane, ts), body, open),
        }
    }

    /// The selected message (of the active pane): its pane, its time and its text as read.
    fn selected_body(&self) -> Option<(PaneId, Ts, Vec<Block>)> {
        let id = self.work.active()?;
        let p = self.work.pane(id)?;
        let tl = self.work.timeline(p);
        let (a, _) = p.range(tl)?;
        Some((id, tl.items[a].ts, tl.items[a].body.clone()))
    }

    /// A click on link `spot` as drawn: it opens as with `o`, or the status line says why it
    /// does not (the message changed since: its link there is no longer the one drawn).
    pub(super) fn open_clicked(&mut self, id: PaneId, spot: &LinkSpot, now: Instant) {
        let Some(p) = self.work.pane(id) else { return };
        let tl = self.work.timeline(p);
        let now_there = tl.position(spot.ts).and_then(|j| links_of(&tl.items[j].body).into_iter().nth(spot.link));
        match now_there.filter(|(url, _)| *url == spot.url) {
            Some((url, label)) => self.open_address(&url, label.as_deref(), now),
            None => self.warn(Msg::Label(Label::OpenChanged), now),
        }
    }

    /// `:open <arg>`: the address typed; without one, as `o`.
    pub(super) fn open_typed(&mut self, arg: &str, now: Instant) {
        if arg.is_empty() { self.open_link(now) } else { self.open_address(arg, None, now) }
    }

    /// Open `url`, a link's address whose text is `label`: refused unless it is a web address
    /// ([`opener::check`], handed over in its checked form); asked about first when the text
    /// names another host; else handed to the binary, the host it goes to said.
    pub(super) fn open_address(&mut self, url: &str, label: Option<&str>, now: Instant) {
        let Some(checked) = opener::check(url) else { return self.warn(Msg::Label(Label::OpenRefused), now) };
        match label.filter(|l| mrkdwn::real_target(url, l).is_some()) {
            Some(said) => {
                let said = clip(said, SAID_MAX);
                self.opening = Some(Opening { url: checked.url, host: checked.host, said });
                self.dialog = Some(Dialog::new(Question::Open));
            }
            None => {
                self.effects.push(Effect::OpenLink(checked.url));
                self.say_opening(&checked.host, now);
            }
        }
    }

    /// The answer to "open this link?": yes hands it to the binary, the host it goes to said.
    pub(super) fn answer_open(&mut self, yes: bool, now: Instant) {
        if let Some(o) = self.opening.take().filter(|_| yes) {
            self.effects.push(Effect::OpenLink(o.url));
            self.say_opening(&o.host, now);
        }
    }

    /// Say the link to `host` is opening. A notice that no link opened, still showing from a
    /// click or key before, is no longer true: this one takes its place. The end of a long host
    /// is what says whose it is: it is cut from its start, to the room the status line has.
    fn say_opening(&mut self, host: &str, now: Instant) {
        let before = |m: &Msg| matches!(m, Msg::Label(Label::OpenRefused | Label::OpenChanged | Label::OpenNone));
        self.status.forget(now, before);
        let around = crate::text::width(&self.i18n.msg(&Msg::OpenOpening { host: String::new() }));
        let room = if self.size.width < WIDE { HOST_SAID } else { NOTICE_MAX.saturating_sub(around).max(HOST_SAID) };
        self.info(Msg::OpenOpening { host: opener::host_tail(host, room) }, now);
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
