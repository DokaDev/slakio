//! Pictures in the timeline: previews of the pictures messages share, under their cards,
//! people's profile photos in the comfortable layout's picture slot and in the list's DM rows
//! (`avatars = "image"`), and a workspace's own emoji in text and reaction chips ([`custom`],
//! two cells each, [`Shape::Emoji`]), drawn with the kitty graphics protocol where the terminal draws it
//! ([`detect`]); elsewhere, and while a picture is not there yet, the text card and the initials
//! stay as they are. A picture is kept per address and the cells it fills ([`Shape`]): a photo
//! shown in both places is two pictures to the terminal, fetched once each (at most two fetches
//! a run while the screen does not change and neither is removed or fails).
//!
//! How a picture comes: drawing a frame notes the pictures it would show ([`Images::look`]);
//! after the frame the app asks the backend for those it does not have ([`Images::ask`], at most
//! [`IN_FLIGHT_MAX`] at once; what a frame only laid out to measure it is not); the bytes are
//! decoded and made into the commands that send them off the loop's thread (the binary runs
//! [`prepare`] on a blocking task); the result is sent to the terminal once
//! ([`Images::take_graphics`], written before the next frame) and drawn as placeholder cells
//! from then on ([`graphics`]).
//!
//! What the terminal holds for slakio is bounded: at most [`ENTRIES_MAX`] pictures and
//! [`BYTES_CAP`] bytes of them; past that the least recently drawn is removed from the terminal
//! (and fetched again if it is drawn again), but never one the latest frame or the one before it
//! looked at: when every picture held is on screen, a list photo makes room for a timeline
//! picture, and otherwise the new picture is not sent and waits, parked, until the screen
//! changes (the set of pictures a shown frame looks at differs from the frame before's). While
//! the pictures on screen fill the terminal's room, nothing more is asked for. A frame draws at
//! most [`EMOJI_PER_FRAME`] distinct emoji pictures ([`Images::look_emoji`]), counting only
//! what it shows ([`Images::not_shown`]); the rest stay text. The timeline's pictures are asked for before the list's photos, which yield their room
//! to them. A picture that failed says why on its card (too large, unreadable, not loaded); one
//! that failed for the network or the request budget is asked again after [`RETRY_AFTER`]
//! ([`RETRY_SOON`] for a busy or offline backend), when it is drawn again; at most
//! [`FAILED_MAX`] failures are kept.

pub mod custom;
pub mod decode;
pub mod detect;
pub mod graphics;
mod note;
mod wake;

use crate::app::requests::Requests;
use decode::{Refused, Shape};
use slakio_core::backend::{Command, FailKind, Generation};
use slakio_core::diag::{Diag, Stage};
use slakio_core::images::{BYTES_MAX, Source};
use slakio_core::model::WorkspaceId;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

/// The most pictures asked for or being decoded at once.
pub const IN_FLIGHT_MAX: usize = 4;
/// The most pictures the terminal holds for slakio.
pub const ENTRIES_MAX: usize = 64;
/// The most distinct emoji pictures one frame draws (the others stay `:name:`): well below
/// [`ENTRIES_MAX`], so emoji never fill the terminal's room alone.
pub const EMOJI_PER_FRAME: usize = ENTRIES_MAX / 2;
/// The most bytes (PNG, as sent) of the pictures the terminal holds for slakio.
pub const BYTES_CAP: usize = 24 * 1024 * 1024;
/// How long a picture that failed for the network or the request budget waits to be asked again.
pub const RETRY_AFTER: Duration = Duration::from_secs(60);
/// How long one that found the backend busy or offline waits (that passes quickly).
pub const RETRY_SOON: Duration = Duration::from_secs(2);
/// The most pictures that failed whose reason is kept (the oldest is forgotten, and asked for
/// again when it is drawn again).
pub const FAILED_MAX: usize = 1024;

/// The slot a profile photo fills (the comfortable layout's).
pub const PHOTO: Shape =
    Shape::Photo { cols: crate::avatar::BLOCK.cols as u16, rows: crate::avatar::BLOCK.rows as u16 };
/// The cells a DM's peer's photo fills in the list: the initials chip's.
pub const CHIP_PHOTO: Shape =
    Shape::Photo { cols: crate::avatar::CHIP.cols as u16, rows: crate::avatar::CHIP.rows as u16 };

/// A picture as slakio keeps it: where it comes from and the cells it fills.
type Key = (Source, Shape);

/// Why a picture is not shown, as its card says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// Larger than slakio reads.
    TooLarge,
    /// Not a picture slakio reads.
    Unreadable,
    /// It could not be fetched (gone, refused, the network).
    NotLoaded,
}

/// What a picture is, for drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// In the terminal as picture `id`, `cols` × `rows` cells.
    Ready { id: u32, cols: u16, rows: u16 },
    /// It will not be shown, for this reason.
    Failed(Why),
    /// Not here (yet): draw what stands in for it.
    Pending,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    Asked(Generation),
    /// The bytes of request `.0` are being decoded.
    Decoding(Generation),
    Ready {
        id: u32,
        cols: u16,
        rows: u16,
        bytes: usize,
    },
    Failed {
        why: Why,
        again: Option<Instant>,
    },
    /// Fetched while every picture held was on screen: not sent, and not asked again until the
    /// screen changes (drawn as not here yet meanwhile).
    Parked,
}

/// A picture to decode and make ready for the terminal, for the binary to run off the loop's
/// thread ([`prepare`]): the bytes of request `request`, to be sent as picture `id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub source: Source,
    pub request: Generation,
    pub id: u32,
    pub bytes: Vec<u8>,
    pub shape: Shape,
}

/// A picture ready for the terminal: the commands that send it (base64 and all, made off the
/// loop's thread), its cells and its size as sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub id: u32,
    pub cols: u16,
    pub rows: u16,
    pub bytes: usize,
    pub transmit: String,
    /// What it was read from: width and height in pixels, format (the diagnostics say it).
    pub from: (u32, u32, &'static str),
}

/// `job` decoded ([`decode::decode`], for cells of `cell` pixels) and made into the commands
/// that send it. Blocking work: never on the loop's thread.
pub fn prepare(job: &Job, cell: Option<(u16, u16)>) -> Result<Prepared, Refused> {
    let d = decode::decode(&job.bytes, job.shape, cell)?;
    let (cols, rows) = (d.cols.min(graphics::MARKS), d.rows.min(graphics::MARKS));
    let transmit = graphics::transmit(job.id, cols, rows, &d.png);
    Ok(Prepared { id: job.id, cols, rows, bytes: d.png.len(), transmit, from: d.from })
}

/// What `:images` changed, for the app to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chosen {
    /// As asked.
    Done,
    /// On as asked, though slakio cannot tell that the terminal shows pictures.
    Forced,
    /// `auto` as asked, which here is off: slakio cannot tell that the terminal shows them.
    NotHere,
    /// Kept for later: [`detect::IMAGES_ENV`] decides for this run (its value).
    Pinned(String),
}

/// A place in what frames note ([`Images::mark`]).
pub type Mark = (usize, usize, usize, usize);

/// The emoji pictures the frame being drawn looked at: how many of its looks at each still
/// count, and every look in order (`false` once its message was only laid out).
#[derive(Debug, Default)]
struct EmojiShare {
    live: HashMap<Key, u32>,
    looks: Vec<(Key, bool)>,
}

/// The pictures: what is asked, decoded, held by the terminal, or failed.
#[derive(Debug, Default)]
pub struct Images {
    /// Pictures are drawn at all ([`Images::resolve`]); off, nothing here is ever used.
    pub on: bool,
    /// What decides [`Images::on`]: the environment read at the start, the `images` setting
    /// (the config file's, or the one `:images` chose), the demo, standard output a terminal.
    pub env: detect::Env,
    pub setting: String,
    pub demo: bool,
    pub tty: bool,
    /// The last setting other than `off` (`:images` alone goes back to it).
    pub last_on: Option<String>,
    entries: HashMap<Key, Entry>,
    /// The pictures the terminal holds, the least recently drawn first.
    held: VecDeque<Key>,
    bytes: usize,
    /// The pictures that failed, the oldest first (at most [`FAILED_MAX`]).
    failed: VecDeque<Key>,
    asked: HashMap<Generation, Key>,
    /// What the last frame would have shown and did not have (and its workspace); `false`: it
    /// was only laid out, not shown.
    wanted: RefCell<Vec<(WorkspaceId, Key, bool)>>,
    /// What the last frame looked at, whatever it was (`false`: only laid out).
    drawn: RefCell<Vec<(Key, bool)>>,
    /// What the latest shown frame looked at, and the one before it: never removed.
    screen: HashSet<Key>,
    last_screen: HashSet<Key>,
    /// The distinct emoji pictures the frame being drawn looked at for what it shows (at most
    /// [`EMOJI_PER_FRAME`]).
    emoji_frame: RefCell<EmojiShare>,
    last_id: u32,
    /// The highest picture id ever sent.
    pub highest: u32,
    /// Commands for the terminal not written yet.
    graphics: String,
    /// The workspaces' own emoji: what each name draws (kept whether pictures are on or not).
    pub customs: custom::Customs,
    /// The picture diagnostics (off unless asked for, [`Images::set_diag`]).
    diag: Diag,
    /// The last time the loop woke for a failed picture's or list's wait ([`Images::woke`]).
    woken: Option<Instant>,
}

impl Images {
    /// Write the picture diagnostics to `diag` (the emoji lists' too).
    pub fn set_diag(&mut self, diag: Diag) {
        self.customs.diag = diag.clone();
        self.diag = diag;
    }

    /// [`Images::on`] from what decides it ([`detect::images_on`]). Turned off, every picture
    /// leaves the terminal at once (each by its id, then all) and nothing asked is waited for.
    pub fn resolve(&mut self) {
        let on = self.tty && detect::images_on(&self.env, &self.setting, self.demo);
        if self.on && !on {
            for source in &self.held {
                if let Some(Entry::Ready { id, .. }) = self.entries.get(source) {
                    self.graphics.push_str(&graphics::delete(*id));
                }
            }
            self.graphics.push_str(graphics::delete_all());
            self.entries.clear();
            self.held.clear();
            self.failed.clear();
            self.asked.clear();
            self.bytes = 0;
            self.wanted.take();
            self.drawn.take();
            self.screen.clear();
            self.last_screen.clear();
            self.emoji_frame.take();
        }
        self.on = on;
    }

    /// `:images <value>` (`auto`, `on`, `off`, checked): the setting from now on.
    pub fn choose(&mut self, value: &str) -> Chosen {
        if self.setting != "off" && !self.setting.is_empty() {
            self.last_on = Some(self.setting.clone());
        }
        self.setting = value.to_string();
        self.resolve();
        match self.env.images.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            Some(pinned) if pinned != value => Chosen::Pinned(pinned.to_string()),
            _ if value == "on" && !(self.tty && self.env.terminal_draws()) => Chosen::Forced,
            _ if value == "auto" && !self.on => Chosen::NotHere,
            _ => Chosen::Done,
        }
    }

    /// Profile photos are no longer drawn: those held leave the terminal, and those asked for
    /// or being decoded are not waited for.
    pub fn forget_photos(&mut self) {
        self.forget_where(|k| matches!(k.0, Source::Avatar(_)));
    }

    /// Workspace `ws` is signed in to as someone else: its own emoji go, the list and the
    /// pictures it named (held ones leave the terminal); both are read again when next wanted.
    pub fn forget_customs(&mut self, ws: &WorkspaceId) {
        let gone: HashSet<Source> = self.customs.pictures(ws).into_iter().map(Source::Emoji).collect();
        self.customs.forget(ws);
        self.forget_where(|k| gone.contains(&k.0));
    }

    /// The pictures `gone` says go: those held leave the terminal, and those asked for or being
    /// decoded are not waited for.
    fn forget_where(&mut self, gone: impl Fn(&Key) -> bool) {
        let keys: Vec<Key> = self.entries.keys().filter(|k| gone(k)).cloned().collect();
        self.held.retain(|k| !gone(k));
        self.failed.retain(|k| !gone(k));
        self.asked.retain(|_, k| !gone(k));
        for key in keys {
            if let Some(Entry::Ready { id, bytes, .. }) = self.entries.remove(&key) {
                self.bytes -= bytes;
                self.graphics.push_str(&graphics::delete(id));
            }
        }
    }

    /// What `source` (of workspace `ws`) is for drawing, as a preview or in the comfortable
    /// layout's photo slot ([`Images::look_as`]).
    pub fn look(&self, ws: &WorkspaceId, source: &Source) -> Look {
        let shape = match source {
            Source::Thumb(_) => Shape::Preview,
            Source::Avatar(_) => PHOTO,
            Source::Emoji(_) => Shape::Emoji,
        };
        self.look_as(ws, source, shape)
    }

    /// What `source` (of workspace `ws`) is for drawing as `shape`; noted as wanted when it is
    /// not here.
    pub fn look_as(&self, ws: &WorkspaceId, source: &Source, shape: Shape) -> Look {
        if !self.on {
            return Look::Pending;
        }
        let key = (source.clone(), shape);
        self.drawn.borrow_mut().push((key.clone(), true));
        match self.entries.get(&key) {
            Some(Entry::Ready { id, cols, rows, .. }) => {
                note::once(&self.diag, &key, Stage::Drawn, "ok", &[]);
                Look::Ready { id: *id, cols: *cols, rows: *rows }
            }
            Some(Entry::Failed { why, again }) => {
                let why = *why;
                if again.is_some() {
                    self.wanted.borrow_mut().push((ws.clone(), key, true));
                }
                Look::Failed(why)
            }
            Some(Entry::Asked(_) | Entry::Decoding(_) | Entry::Parked) => Look::Pending,
            None => {
                self.wanted.borrow_mut().push((ws.clone(), key, true));
                Look::Pending
            }
        }
    }

    /// What the emoji picture at `address` (of workspace `ws`) is for drawing in two cells
    /// ([`Images::look_as`]); past [`EMOJI_PER_FRAME`] distinct ones in a frame, not looked at
    /// ([`Look::Pending`]: its name stays text). A frame lays a message out once, so its width
    /// and its drawing agree. A message only laid out gives its part of the share back
    /// ([`Images::not_shown`]).
    pub fn look_emoji(&self, ws: &WorkspaceId, address: &str) -> Look {
        if !self.on {
            return Look::Pending;
        }
        let key = (Source::Emoji(address.to_string()), Shape::Emoji);
        {
            let mut share = self.emoji_frame.borrow_mut();
            let EmojiShare { live, looks } = &mut *share;
            if !live.contains_key(&key) && live.len() >= EMOJI_PER_FRAME {
                if self.diag.on() {
                    note::once(&self.diag, &key, Stage::Skipped, "share", &[("share", EMOJI_PER_FRAME.to_string())]);
                }
                return Look::Pending;
            }
            *live.entry(key.clone()).or_default() += 1;
            looks.push((key.clone(), true));
        }
        self.look_as(ws, &key.0, key.1)
    }

    /// `key` was looked at by the latest shown frame or the one before it: it stays.
    fn on_screen(&self, key: &Key) -> bool {
        self.screen.contains(key) || self.last_screen.contains(key)
    }

    /// Where what frames note stands now: what is noted after it, until a later mark, can be
    /// said to be only laid out ([`Images::not_shown`]).
    pub fn mark(&self) -> Mark {
        let looks = self.emoji_frame.borrow().looks.len();
        (self.wanted.borrow().len(), self.drawn.borrow().len(), self.customs.mark(), looks)
    }

    /// What was noted between `from` and `to` was laid out, not shown (a message measured off
    /// screen): it is neither asked for nor counted as drawn, a workspace's own emoji list it
    /// would want is not asked for because of it, and the emoji it looked at no longer count
    /// toward the frame's [`EMOJI_PER_FRAME`] (unless what is shown looked at them too).
    pub fn not_shown(&self, from: Mark, to: Mark) {
        self.customs.not_shown(from.2, to.2);
        let mut share = self.emoji_frame.borrow_mut();
        let EmojiShare { live, looks } = &mut *share;
        for (key, counts) in looks.get_mut(from.3..to.3).into_iter().flatten().filter(|l| l.1) {
            *counts = false;
            if let Some(n) = live.get_mut(key) {
                *n -= 1;
                if *n == 0 {
                    live.remove(key);
                }
            }
        }
        for w in self.wanted.borrow_mut().get_mut(from.0..to.0).into_iter().flatten() {
            w.2 = false;
        }
        for d in self.drawn.borrow_mut().get_mut(from.1..to.1).into_iter().flatten() {
            d.1 = false;
        }
    }

    fn in_flight(&self) -> usize {
        self.entries.values().filter(|e| matches!(e, Entry::Asked(_) | Entry::Decoding(_))).count()
    }

    /// After a frame: what it showed is the most recently drawn, and what it would have shown
    /// is asked for, the timeline's pictures before the list's photos (at most [`IN_FLIGHT_MAX`]
    /// at once, and none while the pictures on screen and those on their way fill the
    /// terminal's room; a failed one once its wait is over); what it only laid out is neither.
    /// When what it showed differs from the frame before, the parked pictures may be asked
    /// again. `true` when something was asked.
    pub fn ask(&mut self, requests: &mut Requests, now: Instant) -> bool {
        let mut screen = HashSet::new();
        for (key, _) in self.drawn.take().into_iter().filter(|d| d.1) {
            if let Some(i) = self.held.iter().position(|k| *k == key) {
                let s = self.held.remove(i).expect("found");
                self.held.push_back(s);
            }
            screen.insert(key);
        }
        if screen != self.screen {
            self.entries.retain(|_, e| *e != Entry::Parked);
        }
        self.last_screen = std::mem::replace(&mut self.screen, screen);
        self.emoji_frame.take();
        let mut asked = false;
        let mut wanted: Vec<_> = self.wanted.take().into_iter().filter(|w| w.2).collect();
        wanted.sort_by_key(|w| w.1.1 == CHIP_PHOTO);
        for (ws, key, _) in wanted {
            let in_flight = self.in_flight();
            if in_flight >= IN_FLIGHT_MAX {
                if self.diag.on() {
                    note::once(&self.diag, &key, Stage::Skipped, "in-flight", &[("in_flight", in_flight.to_string())]);
                }
                break;
            }
            // A list photo yields its room to the timeline's pictures, never the other way.
            let chip = key.1 == CHIP_PHOTO;
            let staying = self.held.iter().filter(|k| self.on_screen(k) && (chip || k.1 != CHIP_PHOTO)).count();
            if staying + in_flight >= ENTRIES_MAX {
                if self.diag.on() {
                    note::once(&self.diag, &key, Stage::Skipped, "room", &[("staying", staying.to_string())]);
                }
                continue;
            }
            match self.entries.get(&key) {
                None => {}
                Some(Entry::Failed { again: Some(at), .. }) if *at <= now => {}
                Some(_) => continue,
            }
            if self.diag.on() {
                let again = self.entries.contains_key(&key).to_string();
                note::line(&self.diag, &key, Stage::Asked, "ui", &[("again", again)]);
            }
            let id = requests.ask(Some(ws), Command::Image { source: key.0.clone(), max: BYTES_MAX });
            self.asked.insert(id, key.clone());
            self.failed.retain(|k| *k != key);
            self.entries.insert(key, Entry::Asked(id));
            asked = true;
        }
        asked
    }

    /// The bytes picture request `id` asked for came: the job that makes them ready, when it is
    /// one of these (the picture's id is given now).
    pub fn answered(&mut self, id: Generation, bytes: Vec<u8>) -> Option<Job> {
        let key = self.asked.remove(&id)?;
        if self.entries.get(&key) != Some(&Entry::Asked(id)) {
            return None;
        }
        self.entries.insert(key.clone(), Entry::Decoding(id));
        let (source, shape) = key;
        self.last_id = if self.last_id >= graphics::ID_MAX { 1 } else { self.last_id + 1 };
        Some(Job { source, request: id, id: self.last_id, bytes, shape })
    }

    /// `source` failed for `why`; asked again after `again`, if ever. At most [`FAILED_MAX`]
    /// are kept.
    fn fail(&mut self, key: Key, why: Why, again: Option<Instant>) {
        self.failed.retain(|k| *k != key);
        self.failed.push_back(key.clone());
        self.entries.insert(key, Entry::Failed { why, again });
        while self.failed.len() > FAILED_MAX {
            let Some(old) = self.failed.pop_front() else { break };
            if matches!(self.entries.get(&old), Some(Entry::Failed { .. })) {
                self.entries.remove(&old);
            }
        }
    }

    /// Picture request `id` failed (`kind`) at `now`: `true` when it was one of these.
    pub fn failed(&mut self, id: Generation, kind: &FailKind, now: Instant) -> bool {
        let Some(key) = self.asked.remove(&id) else { return false };
        let why = match kind {
            FailKind::TooLarge => Why::TooLarge,
            FailKind::Api { code } if code == "not_an_image" => Why::Unreadable,
            _ => Why::NotLoaded,
        };
        let again = match kind {
            FailKind::Busy | FailKind::Offline => Some(now + RETRY_SOON),
            k if k.retryable() || *k == FailKind::CapReached => Some(now + RETRY_AFTER),
            _ => None,
        };
        note::line(&self.diag, &key, Stage::Failed, &note::fail_word(kind), &[("retry", note::retry(again, now))]);
        self.fail(key, why, again);
        true
    }

    /// The picture of `job` was made ready (or refused): it is sent to the terminal before the
    /// next frame, and the least recently drawn go while there are too many, never one on
    /// screen but a list photo for a timeline picture; with no room, it is parked. A result for
    /// a request no longer waited for changes nothing.
    pub fn decoded(&mut self, job: (Source, Generation), result: Result<Prepared, Refused>) {
        let (source, request) = job;
        // Request ids are never used twice: the one decoding it is the picture.
        let Some(key) =
            self.entries.iter().find(|(k, e)| k.0 == source && **e == Entry::Decoding(request)).map(|(k, _)| k.clone())
        else {
            return;
        };
        let p = match result {
            Ok(p) => p,
            Err(refused) => {
                let why = match refused {
                    Refused::TooLarge => Why::TooLarge,
                    Refused::Unreadable => Why::Unreadable,
                };
                let result = if why == Why::TooLarge { "decode-too-large" } else { "decode-unreadable" };
                note::line(&self.diag, &key, Stage::Refused, result, &[]);
                return self.fail(key, why, None);
            }
        };
        note::decoded(&self.diag, &key, &p);
        while self.held.len() >= ENTRIES_MAX || (self.bytes + p.bytes > BYTES_CAP && !self.held.is_empty()) {
            let off = self.held.iter().position(|k| !self.on_screen(k));
            let chip = || self.held.iter().position(|k| k.1 == CHIP_PHOTO);
            let Some(i) = off.or_else(|| (key.1 != CHIP_PHOTO).then(chip).flatten()) else {
                // Never sent, so nothing to remove from the terminal.
                let held = [("held", self.held.len().to_string()), ("bytes", self.bytes.to_string())];
                note::line(&self.diag, &key, Stage::Parked, "room", &held);
                self.entries.insert(key, Entry::Parked);
                return;
            };
            let old = self.held.remove(i).expect("found");
            if let Some(Entry::Ready { id, bytes, .. }) = self.entries.remove(&old) {
                self.bytes -= bytes;
                self.graphics.push_str(&graphics::delete(id));
            }
        }
        self.graphics.push_str(&p.transmit);
        self.highest = self.highest.max(p.id);
        self.bytes += p.bytes;
        self.entries.insert(key.clone(), Entry::Ready { id: p.id, cols: p.cols, rows: p.rows, bytes: p.bytes });
        self.held.push_back(key);
    }

    /// The commands for the terminal not written yet (sending and removing pictures).
    pub fn take_graphics(&mut self) -> String {
        std::mem::take(&mut self.graphics)
    }

    /// The pictures the terminal holds and their bytes.
    pub fn held(&self) -> (usize, usize) {
        (self.held.len(), self.bytes)
    }

    /// The pictures whose failure is kept.
    pub fn failures(&self) -> usize {
        self.failed.len()
    }
}

#[cfg(test)]
mod tests;
