//! Picking one of the selected message's links (`o` on one with several that open), as tmux and
//! vimium hint modes do: letters are drawn over the start of each link, and typing them opens
//! that one. Up to twelve links get one key each; past that the last keys start two-letter
//! labels and are never labels themselves, so no label starts another (one key each comes first,
//! for the first links). After a first letter only the labels it starts stay, `Backspace` takes
//! it back, and Shift or Caps Lock changes nothing. The letters are never `j`, `k` or `o`: those
//! are the list's (`o` shows it, `j`/`k` show it too, then move its cursor), so a label means the
//! same link in both. When one of the links is not on screen, the list shows at once (host
//! first); it scrolls with its cursor, and `Enter` opens the row under it. The mouse: a press or
//! the wheel stops the letters (then does what it does); over the list, a click on a row opens it,
//! the wheel moves its cursor, a press outside closes it; moving the mouse changes nothing.

use super::App;
use super::links::links_of;
use super::pane::{Pane, Shown};
use crate::action::{Action, LinksAction};
use crate::keymap::{Ctx, KeyChord};
use crate::mrkdwn::Block;
use crate::opener;
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
    /// The letters that pick it (none past the most there are: the list still has it).
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
    /// The letters typed so far.
    pub typed: String,
    /// Shown as a list: its cursor.
    pub list: Option<usize>,
    /// The list's first row shown, as last moved.
    top: usize,
}

/// The keys of the letters, the easiest first; never a key the picker uses otherwise.
pub const LETTERS: [char; 12] = ['a', 's', 'd', 'f', 'g', 'h', 'l', 'e', 'r', 'w', 'u', 'i'];

/// The labels of `n` links (at most 144): one key each while they last; past twelve, the last
/// keys start two-letter labels and label nothing alone, so no label starts another.
pub fn hint_letters(n: usize) -> Vec<String> {
    let k = LETTERS.len();
    if n <= k {
        return LETTERS[..n].iter().map(char::to_string).collect();
    }
    let prefixes = (n - k).div_ceil(k - 1).min(k);
    let singles = LETTERS[..k - prefixes].iter().map(char::to_string);
    let doubles = LETTERS[k - prefixes..].iter().flat_map(|a| LETTERS.iter().map(move |b| format!("{a}{b}")));
    singles.chain(doubles).take(n).collect()
}

/// Where the list of links to pick goes over `body`, `n` of them: centred, at most 64 wide.
pub fn list_rect(body: Rect, n: usize) -> Rect {
    let w = body.width.saturating_sub(4).min(64);
    let h = (n as u16 + 2).min(body.height);
    Rect::new(body.x + (body.width - w) / 2, body.y + body.height.saturating_sub(h) / 2, w, h)
}

/// The first of `n` rows shown, `rows` at a time, from `top` on: moved as little as it takes
/// for `cursor` to be shown.
fn follow(top: usize, cursor: usize, rows: usize, n: usize) -> usize {
    if rows == 0 {
        return cursor;
    }
    top.min(cursor).max((cursor + 1).saturating_sub(rows)).min(n.saturating_sub(rows))
}

impl Picker {
    /// The list's first row shown, `rows` of them showing.
    pub fn list_top(&self, rows: usize) -> usize {
        follow(self.top, self.list.unwrap_or(0), rows, self.picks.len())
    }

    /// The labels the letters typed still start, and what is left of each to type.
    fn left(&self) -> impl Iterator<Item = (&Pick, &str)> {
        self.picks.iter().filter(|k| !k.hint.is_empty() && k.hint.starts_with(&self.typed)).map(|k| {
            let rest = &k.hint[self.typed.len()..];
            (k, rest)
        })
    }
}

impl App {
    /// Start picking among `open` (the links of `body` that open, with their hosts) of the
    /// message at `ts` in `pane`: letters on them when each one's start was drawn last, else the
    /// list.
    pub(super) fn start_picking(&mut self, (pane, ts): (PaneId, Ts), body: Vec<Block>, open: Vec<(usize, String)>) {
        let links = links_of(&body);
        let letters = hint_letters(open.len()).into_iter().chain(std::iter::repeat(String::new()));
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
        let drawn = self.work.pane(pane).map(|p| p.links.borrow().clone()).unwrap_or_default();
        let shown =
            picks.iter().all(|k| !k.hint.is_empty() && drawn.iter().any(|d| d.ts == ts && d.link == k.link && d.first));
        let list = (!shown).then_some(0);
        self.picker = Some(Picker { pane, ts, body, picks, typed: String::new(), list, top: 0 });
    }

    /// A key while a link is being picked: a letter of the labels (any case), else the picker's
    /// key ([`Ctx::Links`]); any other key stops picking. No key goes on to the rest of the app.
    pub(super) fn pick_key(&mut self, key: KeyChord, now: Instant) {
        let plain = !key.mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let letter = match key.code {
            KeyCode::Char(c) if plain && LETTERS.contains(&c.to_ascii_lowercase()) => Some(c.to_ascii_lowercase()),
            _ => None,
        };
        let Some(c) = letter else {
            let bound = self.keymap.bindings(Ctx::Links).find(|(k, _)| *k == [key]).map(|(_, a)| a);
            match bound {
                Some(Action::Links(a)) => self.links_action(a, now),
                _ => self.picker = None,
            }
            return;
        };
        let Some(mut p) = self.picker.take() else { return };
        p.typed.push(c);
        if let Some(i) = p.picks.iter().position(|k| k.hint == p.typed) {
            return self.pick(&p, i, now);
        }
        // A letter that starts no label stops picking.
        if p.left().next().is_some() {
            self.picker = Some(p);
        }
    }

    /// The picker's keys: the list (shown, moved, its row opened), a letter taken back, stop.
    pub(super) fn links_action(&mut self, a: LinksAction, now: Instant) {
        let rows = usize::from(self.list_rects().map_or(0, |(_, r)| r.height));
        let Some(mut p) = self.picker.take() else { return };
        let last = p.picks.len().saturating_sub(1);
        match (a, p.list) {
            (LinksAction::Cancel, _) => return,
            // Only a row shown opens.
            (LinksAction::Open, Some(i)) if rows > 0 => return self.pick(&p, i, now),
            (LinksAction::List | LinksAction::Down | LinksAction::Up, None) => p.list = Some(0),
            (LinksAction::Down, Some(i)) => p.list = Some((i + 1).min(last)),
            (LinksAction::Up, Some(i)) => p.list = Some(i.saturating_sub(1)),
            (LinksAction::Back, _) => {
                p.typed.pop();
            }
            _ => {}
        }
        p.top = p.list_top(rows);
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

    /// The letters still to type of each link of `m` in `pane`, by the link's number, while
    /// they are drawn on it (empty otherwise); a link whose label the letters typed do not
    /// start gets none.
    pub fn link_hints(&self, pane: &Pane, m: &Shown) -> Vec<Option<String>> {
        let Some(p) = self.picker.as_ref().filter(|p| p.list.is_none() && p.ts == m.ts && p.body == m.body) else {
            return Vec::new();
        };
        if !self.work.pane(p.pane).is_some_and(|q| std::ptr::eq(q, pane)) {
            return Vec::new();
        }
        let mut out = vec![None; p.picks.iter().map(|k| k.link + 1).max().unwrap_or(0)];
        for (k, rest) in p.left() {
            out[k.link] = Some(rest.to_string());
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

    /// The keys the status line offers while a link is picked: the next letters (three at most),
    /// the list, a letter taken back once one is typed, stop.
    pub fn picker_hints(&self) -> Option<Vec<(String, Label)>> {
        let p = self.picker.as_ref()?;
        let esc = ("Esc".to_string(), Label::HintCancel);
        if p.list.is_some() {
            return Some(vec![("j/k".to_string(), Label::HintMove), ("Enter".to_string(), Label::HintOpen), esc]);
        }
        let mut next: Vec<char> = Vec::new();
        for c in p.left().filter_map(|(_, rest)| rest.chars().next()) {
            if !next.contains(&c) {
                next.push(c);
            }
        }
        let mut letters = next.iter().take(3).map(char::to_string).collect::<Vec<_>>().join("/");
        if next.len() > 3 {
            letters.push('…');
        }
        let mut out = vec![(letters, Label::HintOpen), ("o".to_string(), Label::HintList)];
        if !p.typed.is_empty() {
            out.push(("Backspace".to_string(), Label::HintBack));
        }
        out.push(esc);
        Some(out)
    }

    /// Where the list is, and its rows, while it shows.
    fn list_rects(&self) -> Option<(Rect, Rect)> {
        let p = self.picker.as_ref().filter(|p| p.list.is_some())?;
        let body = Rect { height: self.size.height.saturating_sub(1), ..self.size };
        let rect = list_rect(body, p.picks.len());
        Some((rect, crate::screen::inner(rect)))
    }

    /// The mouse while a link is picked. `Some(redraw)`: the list took it. `None`: the rest of
    /// the app has it; while letters show, a press or the wheel stops picking first (the caller
    /// draws again), a move or a release changes nothing.
    pub(super) fn pick_mouse(&mut self, kind: MouseEventKind, at: Position, now: Instant) -> Option<bool> {
        let list = self.picker.as_ref()?.list;
        let motion = matches!(kind, MouseEventKind::Moved | MouseEventKind::Up(_) | MouseEventKind::Drag(_));
        let Some(cursor) = list else {
            if !motion {
                self.picker = None;
            }
            return None;
        };
        let (rect, inner) = self.list_rects()?;
        match kind {
            _ if motion => return Some(false),
            MouseEventKind::ScrollDown => self.links_action(LinksAction::Down, now),
            MouseEventKind::ScrollUp => self.links_action(LinksAction::Up, now),
            MouseEventKind::Down(MouseButton::Left) if inner.contains(at) => {
                let p = self.picker.take()?;
                let i = follow(p.top, cursor, usize::from(inner.height), p.picks.len()) + usize::from(at.y - inner.y);
                if i < p.picks.len() {
                    self.pick(&p, i, now);
                }
            }
            // On the list's border: nothing.
            MouseEventKind::Down(MouseButton::Left) if rect.contains(at) => return Some(false),
            _ => self.picker = None,
        }
        Some(true)
    }
}

/// The links of `body` that open, with the host each goes to, by their number.
pub(super) fn openable(body: &[Block]) -> Vec<(usize, String)> {
    links_of(body).iter().enumerate().filter_map(|(i, (url, _))| opener::openable(url).map(|h| (i, h))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_one_key_first_and_none_starts_another() {
        for n in 1..=144 {
            let h = hint_letters(n);
            assert_eq!(h.len(), n);
            for (i, a) in h.iter().enumerate() {
                assert!(a.chars().all(|c| LETTERS.contains(&c)), "{n}: {a}");
                assert!(
                    h.iter().enumerate().all(|(j, b)| i == j || !b.starts_with(a.as_str())),
                    "{n}: {a} starts another"
                );
            }
            // One key each before any two.
            let singles = h.iter().take_while(|a| a.len() == 1).count();
            assert!(h[singles..].iter().all(|a| a.len() == 2), "{n}: {h:?}");
        }
        assert_eq!(hint_letters(12).iter().filter(|a| a.len() == 1).count(), 12);
        assert_eq!(hint_letters(13)[..12].iter().filter(|a| a.len() == 1).count(), 11);
        assert!(!LETTERS.iter().any(|c| matches!(c, 'j' | 'k' | 'o')));
    }

    #[test]
    fn the_list_follows_its_cursor() {
        assert_eq!(follow(0, 25, 21, 30), 5);
        assert_eq!(follow(5, 3, 21, 30), 3);
        assert_eq!(follow(9, 29, 21, 30), 9);
        assert_eq!(follow(20, 29, 21, 30), 9, "never past the last rows");
    }
}
