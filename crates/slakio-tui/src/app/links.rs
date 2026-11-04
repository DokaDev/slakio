//! What the selected message holds, taken out of the app: its text copied (`y`), its links
//! opened in the browser (`o`, `:open`, a click on one, `:open <address>` typed). With several
//! links that open, `o` draws letters on each (home-row keys, two of them past nine links) and
//! the letters typed open that one; when one of them is not on screen, the list of them shows
//! instead (host first). Opening is a local action — nothing goes to Slack — so a read-only
//! workspace opens links too. Only a web address opens ([`opener::check`]): any other link gets
//! no letters, and a click on one says why. One whose text names another host than it goes to
//! is asked about first, the real host shown.

use super::dialog::{Dialog, Question};
use super::pane::{Pane, Shown};
use super::{App, Effect};
use crate::keymap::KeyChord;
use crate::mrkdwn::{self, Block, Piece};
use crate::opener;
use crate::text::clip;
use ratatui::crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::layout::{Position, Rect};
use slakio_core::i18n::{Label, Msg};
use slakio_core::layout::PaneId;
use slakio_core::model::Ts;
use std::time::Instant;

/// A link that opens, of the message whose links are picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub url: String,
    pub label: Option<String>,
    /// Where it goes.
    pub host: String,
    /// The letters that pick it.
    pub hint: String,
    /// Its number among the message's links.
    pub link: usize,
}

/// The links of a message being picked (`o` on one with several that open).
#[derive(Clone, Debug)]
pub struct Picker {
    pane: PaneId,
    ts: Ts,
    /// The message as read when picking began: the letters show, and a pick opens, only while
    /// it reads the same.
    body: Vec<Block>,
    pub picks: Vec<Pick>,
    typed: String,
    /// Shown as a list (a link was not on screen): its cursor.
    pub list: Option<usize>,
}

/// The keys of the letters, home row first.
const LETTERS: [char; 9] = ['a', 's', 'd', 'f', 'j', 'k', 'l', 'g', 'h'];

/// The letters of `n` links: one key each up to nine, else two each (no one a start of another).
fn hint_letters(n: usize) -> Vec<String> {
    if n <= LETTERS.len() {
        return LETTERS.iter().take(n).map(char::to_string).collect();
    }
    LETTERS.iter().flat_map(|a| LETTERS.iter().map(move |b| format!("{a}{b}"))).take(n).collect()
}

/// The links of a message's text (code blocks left out), in their order: each one's address
/// and its text, if any. Drawing numbers them the same way.
fn links_of(body: &[Block]) -> Vec<(String, Option<String>)> {
    let pieces = body.iter().flat_map(|b| match b {
        Block::Line { pieces, .. } => pieces.as_slice(),
        Block::Code(_) => &[],
    });
    pieces
        .filter_map(|p| if let Piece::Link { url, label, .. } = p { Some((url.clone(), label.clone())) } else { None })
        .collect()
}

/// Where the list of links to pick goes over `body`, `n` of them: centred, at most 64 wide.
pub fn list_rect(body: Rect, n: usize) -> Rect {
    let w = body.width.saturating_sub(4).min(64);
    let h = (n as u16 + 2).min(body.height);
    Rect::new(body.x + (body.width - w) / 2, body.y + body.height.saturating_sub(h) / 2, w, h)
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

    /// `o` (`:open`): the selected message's link, when one opens; when several do, each gets
    /// the letters that pick it, drawn on it ([`Picker`]), or, when one of them is not on
    /// screen, the list of them. A message whose links all are refused says why.
    pub(super) fn open_link(&mut self, now: Instant) {
        let Some((pane, ts, body)) = self.selected_body() else { return self.info(Msg::Label(Label::OpenNone), now) };
        let links = links_of(&body);
        let open: Vec<(usize, String)> =
            links.iter().enumerate().filter_map(|(i, (url, _))| opener::openable(url).map(|h| (i, h))).collect();
        if open.is_empty() {
            let why = if links.is_empty() { Label::OpenNone } else { Label::OpenRefused };
            return self.warn(Msg::Label(why), now);
        }
        if let [(i, _)] = open[..] {
            let (url, label) = &links[i];
            return self.open_address(url, label.as_deref(), now);
        }
        let letters = hint_letters(open.len());
        let picks: Vec<Pick> = open
            .into_iter()
            .zip(letters)
            .map(|((link, host), hint)| Pick {
                url: links[link].0.clone(),
                label: links[link].1.clone(),
                host,
                hint,
                link,
            })
            .collect();
        // Each one's start was drawn last: the letters go there; else the list.
        let drawn = self.work.pane(pane).map(|p| p.links.borrow().clone()).unwrap_or_default();
        let shown = picks.iter().all(|k| drawn.iter().any(|d| d.ts == ts && d.link == k.link && d.first));
        self.picker = Some(Picker { pane, ts, body, picks, typed: String::new(), list: (!shown).then_some(0) });
    }

    /// The selected message (of the active pane): its pane, its time and its text as read.
    fn selected_body(&self) -> Option<(PaneId, Ts, Vec<Block>)> {
        let id = self.work.active()?;
        let p = self.work.pane(id)?;
        let tl = self.work.timeline(p);
        let (a, _) = p.range(tl)?;
        Some((id, tl.items[a].ts, tl.items[a].body.clone()))
    }

    /// A key while a link is being picked: its letters open it, `o` lists them, in the list
    /// `j`/`k` (or the arrows) move and `Enter` opens; `Esc`, or any other key, stops picking.
    pub(super) fn pick_key(&mut self, key: KeyChord, now: Instant) {
        let Some(mut p) = self.picker.take() else { return };
        let last = p.picks.len().saturating_sub(1);
        let typed = match key.code {
            KeyCode::Char(c) if !key.mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => Some(c),
            _ => None,
        };
        match (key.code, typed, p.list) {
            (KeyCode::Enter, _, Some(i)) => return self.pick(&p, i, now),
            (KeyCode::Down, _, Some(i)) | (_, Some('j'), Some(i)) => p.list = Some((i + 1).min(last)),
            (KeyCode::Up, _, Some(i)) | (_, Some('k'), Some(i)) => p.list = Some(i.saturating_sub(1)),
            (_, Some('o'), None) => p.list = Some(0),
            (_, Some(c), _) => {
                p.typed.push(c);
                if let Some(i) = p.picks.iter().position(|k| k.hint == p.typed) {
                    return self.pick(&p, i, now);
                }
                if !p.picks.iter().any(|k| k.hint.starts_with(&p.typed)) {
                    return;
                }
            }
            _ => return,
        }
        self.picker = Some(p);
    }

    /// Open pick `i` of `p`, the message read as it was when picking began (else it says so:
    /// the letters drawn may no longer be on that link).
    fn pick(&mut self, p: &Picker, i: usize, now: Instant) {
        let same = self.work.pane(p.pane).is_some_and(|pane| {
            let tl = self.work.timeline(pane);
            tl.position(p.ts).is_some_and(|j| tl.items[j].body == p.body)
        });
        match p.picks.get(i).filter(|_| same) {
            Some(k) => self.open_address(&k.url, k.label.as_deref(), now),
            None => self.warn(Msg::Label(Label::OpenChanged), now),
        }
    }

    /// A click on link `link` of the message at `ts` in pane `id`, as drawn: it opens as with
    /// `o`, or the status line says why it does not.
    pub(super) fn open_clicked(&mut self, id: PaneId, ts: Ts, link: usize, now: Instant) {
        let Some(p) = self.work.pane(id) else { return };
        let tl = self.work.timeline(p);
        let Some((url, label)) = tl.position(ts).and_then(|j| links_of(&tl.items[j].body).into_iter().nth(link)) else {
            return;
        };
        self.open_address(&url, label.as_deref(), now);
    }

    /// `:open <arg>`: the address typed; without one, as `o`.
    pub(super) fn open_typed(&mut self, arg: &str, now: Instant) {
        if arg.is_empty() { self.open_link(now) } else { self.open_address(arg, None, now) }
    }

    /// The letters that pick each link of `m` in `pane`, by the link's number, while they are
    /// drawn on it (empty otherwise).
    pub fn link_hints(&self, pane: &Pane, m: &Shown) -> Vec<Option<String>> {
        let Some(p) = self.picker.as_ref().filter(|p| p.list.is_none() && p.ts == m.ts && p.body == m.body) else {
            return Vec::new();
        };
        if !self.work.pane(p.pane).is_some_and(|q| std::ptr::eq(q, pane)) {
            return Vec::new();
        }
        let mut out = vec![None; p.picks.iter().map(|k| k.link + 1).max().unwrap_or(0)];
        for k in &p.picks {
            out[k.link] = Some(k.hint.clone());
        }
        out
    }

    /// The message of `pane` whose links show their letters, if one does.
    pub fn picking_in(&self, pane: &Pane) -> Option<Ts> {
        let p = self.picker.as_ref().filter(|p| p.list.is_none())?;
        self.work.pane(p.pane).filter(|q| std::ptr::eq(*q, pane)).map(|_| p.ts)
    }

    /// The links being picked, while they are.
    pub fn picker(&self) -> Option<&Picker> {
        self.picker.as_ref()
    }

    /// The keys the status line offers while a link is picked.
    pub fn picker_hints(&self) -> Option<Vec<(String, Label)>> {
        let p = self.picker.as_ref()?;
        let esc = ("Esc".to_string(), Label::HintCancel);
        Some(match p.list {
            None => vec![("o".to_string(), Label::HintPickLink), esc],
            Some(_) => vec![("j/k".to_string(), Label::HintMove), ("Enter".to_string(), Label::HintOpen), esc],
        })
    }

    /// The mouse while a link is picked: in the list, a click on a row opens it; anything else
    /// stops picking. `true` when the mouse was taken.
    pub(super) fn pick_mouse(&mut self, kind: MouseEventKind, at: Position, now: Instant) -> bool {
        let Some(p) = self.picker.take() else { return false };
        let Some(_) = p.list else { return false };
        let body = Rect { height: self.size.height.saturating_sub(1), ..self.size };
        let inner = crate::screen::inner(list_rect(body, p.picks.len()));
        if kind == MouseEventKind::Down(MouseButton::Left) && inner.contains(at) {
            self.pick(&p, usize::from(at.y - inner.y), now);
        }
        true
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

    /// The answer to "open this link?": yes hands it to the binary, the host it goes to said.
    pub(super) fn answer_open(&mut self, yes: bool, now: Instant) {
        if let Some(o) = self.opening.take().filter(|_| yes) {
            self.effects.push(Effect::OpenLink(o.url));
            self.info(Msg::OpenOpening { host: opener::host_tail(&o.host, HOST_SAID) }, now);
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
