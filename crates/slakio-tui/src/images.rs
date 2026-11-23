//! Pictures in the timeline: previews of the pictures messages share, under their cards, and
//! people's profile photos in the comfortable layout's picture slot and in the list's DM rows
//! (`avatars = "image"`), drawn with the kitty graphics protocol where the terminal draws it
//! ([`detect`]); elsewhere, and while a picture is not there yet, the text card and the initials
//! stay as they are. A picture is kept per address and the cells it fills ([`Shape`]): a photo
//! shown in both places is two pictures to the terminal.
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
//! (and fetched again if it is drawn again). A picture that failed says why on its card (too
//! large, unreadable, not loaded); one that failed for the network or the request budget is
//! asked again after [`RETRY_AFTER`] ([`RETRY_SOON`] for a busy or offline backend), when it is
//! drawn again; at most [`FAILED_MAX`] failures are kept.

pub mod decode;
pub mod detect;
pub mod graphics;

use crate::app::requests::Requests;
use decode::{Refused, Shape};
use slakio_core::backend::{Command, FailKind, Generation};
use slakio_core::images::{BYTES_MAX, Source};
use slakio_core::model::WorkspaceId;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// The most pictures asked for or being decoded at once.
pub const IN_FLIGHT_MAX: usize = 4;
/// The most pictures the terminal holds for slakio.
pub const ENTRIES_MAX: usize = 64;
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
}

/// `job` decoded ([`decode::decode`], for cells of `cell` pixels) and made into the commands
/// that send it. Blocking work: never on the loop's thread.
pub fn prepare(job: &Job, cell: Option<(u16, u16)>) -> Result<Prepared, Refused> {
    let d = decode::decode(&job.bytes, job.shape, cell)?;
    let (cols, rows) = (d.cols.min(graphics::MARKS), d.rows.min(graphics::MARKS));
    let transmit = graphics::transmit(job.id, cols, rows, &d.png);
    Ok(Prepared { id: job.id, cols, rows, bytes: d.png.len(), transmit })
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
pub type Mark = (usize, usize);

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
    /// What the last frame showed (`false`: only laid out).
    drawn: RefCell<Vec<(Key, bool)>>,
    last_id: u32,
    /// The highest picture id ever sent.
    pub highest: u32,
    /// Commands for the terminal not written yet.
    graphics: String,
}

impl Images {
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
        let photo = |k: &Key| matches!(k.0, Source::Avatar(_));
        let photos: Vec<Key> = self.entries.keys().filter(|k| photo(k)).cloned().collect();
        self.held.retain(|k| !photo(k));
        self.failed.retain(|k| !photo(k));
        self.asked.retain(|_, k| !photo(k));
        for key in photos {
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
            Source::Avatar(_) | Source::Emoji(_) => PHOTO,
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
        match self.entries.get(&key) {
            Some(Entry::Ready { id, cols, rows, .. }) => {
                let look = Look::Ready { id: *id, cols: *cols, rows: *rows };
                self.drawn.borrow_mut().push((key, true));
                look
            }
            Some(Entry::Failed { why, again }) => {
                let why = *why;
                if again.is_some() {
                    self.wanted.borrow_mut().push((ws.clone(), key, true));
                }
                Look::Failed(why)
            }
            Some(Entry::Asked(_) | Entry::Decoding(_)) => Look::Pending,
            None => {
                self.wanted.borrow_mut().push((ws.clone(), key, true));
                Look::Pending
            }
        }
    }

    /// Where what frames note stands now: what is noted after it, until a later mark, can be
    /// said to be only laid out ([`Images::not_shown`]).
    pub fn mark(&self) -> Mark {
        (self.wanted.borrow().len(), self.drawn.borrow().len())
    }

    /// What was noted between `from` and `to` was laid out, not shown (a message measured off
    /// screen): it is neither asked for nor counted as drawn.
    pub fn not_shown(&self, from: Mark, to: Mark) {
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
    /// is asked for (at most [`IN_FLIGHT_MAX`] at once; a failed one once its wait is over); what
    /// it only laid out is neither. `true` when something was asked.
    pub fn ask(&mut self, requests: &mut Requests, now: Instant) -> bool {
        for (key, _) in self.drawn.take().into_iter().filter(|d| d.1) {
            if let Some(i) = self.held.iter().position(|k| *k == key) {
                let s = self.held.remove(i).expect("found");
                self.held.push_back(s);
            }
        }
        let mut asked = false;
        for (ws, key, _) in self.wanted.take().into_iter().filter(|w| w.2) {
            if self.in_flight() >= IN_FLIGHT_MAX {
                break;
            }
            match self.entries.get(&key) {
                None => {}
                Some(Entry::Failed { again: Some(at), .. }) if *at <= now => {}
                Some(_) => continue,
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
        self.fail(key, why, again);
        true
    }

    /// The picture of `job` was made ready (or refused): it is sent to the terminal before the
    /// next frame, and the least recently drawn go while there are too many. A result for a
    /// request no longer waited for changes nothing.
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
                return self.fail(key, why, None);
            }
        };
        self.graphics.push_str(&p.transmit);
        self.highest = self.highest.max(p.id);
        self.bytes += p.bytes;
        self.entries.insert(key.clone(), Entry::Ready { id: p.id, cols: p.cols, rows: p.rows, bytes: p.bytes });
        self.held.push_back(key);
        while self.held.len() > ENTRIES_MAX || (self.bytes > BYTES_CAP && self.held.len() > 1) {
            let Some(old) = self.held.pop_front() else { break };
            if let Some(Entry::Ready { id, bytes, .. }) = self.entries.remove(&old) {
                self.bytes -= bytes;
                self.graphics.push_str(&graphics::delete(id));
            }
        }
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
