//! Pictures in the timeline: previews of the pictures messages share, under their cards, and
//! people's profile photos in the comfortable layout's picture slot (`avatars = "image"`), drawn
//! with the kitty graphics protocol where the terminal draws it ([`detect`]); elsewhere, and
//! while a picture is not there yet, the text card and the initials stay as they are.
//!
//! How a picture comes: drawing a frame notes the pictures it would show ([`Images::look`]);
//! after the frame the app asks the backend for those it does not have ([`Images::ask`], at most
//! [`IN_FLIGHT_MAX`] at once); the bytes are decoded off the loop's thread (the binary runs
//! [`decode::decode`] on a blocking task); the result is sent to the terminal once
//! ([`Images::take_graphics`], written before the next frame) and drawn as placeholder cells
//! from then on ([`graphics`]).
//!
//! What the terminal holds for slakio is bounded: at most [`ENTRIES_MAX`] pictures and
//! [`BYTES_CAP`] bytes of them; past that the least recently drawn is removed from the terminal
//! (and fetched again if it is drawn again). A picture that failed says why on its card (too
//! large, unreadable, not loaded); one that failed for the network or the request budget is
//! asked again after [`RETRY_AFTER`], when it is drawn again.

pub mod decode;
pub mod detect;
pub mod graphics;

use crate::app::requests::Requests;
use decode::{Decoded, Refused, Shape};
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

/// The slot a profile photo fills (the comfortable layout's).
pub const PHOTO: Shape =
    Shape::Photo { cols: crate::avatar::BLOCK.cols as u16, rows: crate::avatar::BLOCK.rows as u16 };

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
    Decoding,
    Ready { id: u32, cols: u16, rows: u16, bytes: usize },
    Failed { why: Why, again: Option<Instant> },
}

/// A picture to decode, for the binary to run off the loop's thread ([`decode::decode`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub source: Source,
    pub bytes: Vec<u8>,
    pub shape: Shape,
}

/// What `:images` changed, for the app to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chosen {
    /// As asked.
    Done,
    /// On as asked, though slakio cannot tell that the terminal shows pictures.
    Forced,
    /// Kept for later: [`detect::IMAGES_ENV`] decides for this run (its value).
    Pinned(String),
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
    entries: HashMap<Source, Entry>,
    /// The pictures the terminal holds, the least recently drawn first.
    held: VecDeque<Source>,
    bytes: usize,
    asked: HashMap<Generation, Source>,
    /// What the last frame would have shown and did not have (and its workspace).
    wanted: RefCell<Vec<(WorkspaceId, Source)>>,
    /// What the last frame showed.
    drawn: RefCell<Vec<Source>>,
    last_id: u32,
    /// Commands for the terminal not written yet.
    graphics: String,
}

impl Images {
    /// [`Images::on`] from what decides it ([`detect::images_on`]). Turned off, every picture
    /// leaves the terminal at once and nothing asked is waited for.
    pub fn resolve(&mut self) {
        let on = self.tty && detect::images_on(&self.env, &self.setting, self.demo);
        if self.on && !on {
            self.graphics.push_str(graphics::delete_all());
            self.entries.clear();
            self.held.clear();
            self.asked.clear();
            self.bytes = 0;
            self.wanted.take();
            self.drawn.take();
        }
        self.on = on;
    }

    /// `:images <value>` (`auto`, `on`, `off`, checked): the setting from now on.
    pub fn choose(&mut self, value: &str) -> Chosen {
        self.setting = value.to_string();
        self.resolve();
        match self.env.images.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            Some(pinned) if pinned != value => Chosen::Pinned(pinned.to_string()),
            _ if value == "on" && !(self.tty && self.env.terminal_draws()) => Chosen::Forced,
            _ => Chosen::Done,
        }
    }

    /// Profile photos are no longer drawn: they leave the terminal.
    pub fn forget_photos(&mut self) {
        let photos: Vec<Source> = self.held.iter().filter(|s| matches!(s, Source::Avatar(_))).cloned().collect();
        self.held.retain(|s| !matches!(s, Source::Avatar(_)));
        for source in photos {
            if let Some(Entry::Ready { id, bytes, .. }) = self.entries.remove(&source) {
                self.bytes -= bytes;
                self.graphics.push_str(&graphics::delete(id));
            }
        }
    }

    /// What `source` (of workspace `ws`) is for drawing; noted as wanted when it is not here.
    pub fn look(&self, ws: &WorkspaceId, source: &Source) -> Look {
        if !self.on {
            return Look::Pending;
        }
        match self.entries.get(source) {
            Some(Entry::Ready { id, cols, rows, .. }) => {
                self.drawn.borrow_mut().push(source.clone());
                Look::Ready { id: *id, cols: *cols, rows: *rows }
            }
            Some(Entry::Failed { why, again }) => {
                if again.is_some() {
                    self.wanted.borrow_mut().push((ws.clone(), source.clone()));
                }
                Look::Failed(*why)
            }
            Some(Entry::Asked(_) | Entry::Decoding) => Look::Pending,
            None => {
                self.wanted.borrow_mut().push((ws.clone(), source.clone()));
                Look::Pending
            }
        }
    }

    fn in_flight(&self) -> usize {
        self.entries.values().filter(|e| matches!(e, Entry::Asked(_) | Entry::Decoding)).count()
    }

    /// After a frame: what it drew is the most recently drawn, and what it wanted is asked for
    /// (at most [`IN_FLIGHT_MAX`] at once; a failed one once its wait is over). `true` when
    /// something was asked.
    pub fn ask(&mut self, requests: &mut Requests, now: Instant) -> bool {
        for source in self.drawn.take() {
            if let Some(i) = self.held.iter().position(|s| *s == source) {
                let s = self.held.remove(i).expect("found");
                self.held.push_back(s);
            }
        }
        let mut asked = false;
        for (ws, source) in self.wanted.take() {
            if self.in_flight() >= IN_FLIGHT_MAX {
                break;
            }
            match self.entries.get(&source) {
                None => {}
                Some(Entry::Failed { again: Some(at), .. }) if *at <= now => {}
                Some(_) => continue,
            }
            let id = requests.ask(Some(ws), Command::Image { source: source.clone(), max: BYTES_MAX });
            self.asked.insert(id, source.clone());
            self.entries.insert(source, Entry::Asked(id));
            asked = true;
        }
        asked
    }

    /// The bytes picture request `id` asked for came: the job that decodes them, when it is one
    /// of these.
    pub fn answered(&mut self, id: Generation, bytes: Vec<u8>) -> Option<Job> {
        let source = self.asked.remove(&id)?;
        if self.entries.get(&source) != Some(&Entry::Asked(id)) {
            return None;
        }
        self.entries.insert(source.clone(), Entry::Decoding);
        let shape = match source {
            Source::Thumb(_) => Shape::Preview,
            Source::Avatar(_) => PHOTO,
        };
        Some(Job { source, bytes, shape })
    }

    /// Picture request `id` failed (`kind`) at `now`: `true` when it was one of these.
    pub fn failed(&mut self, id: Generation, kind: &FailKind, now: Instant) -> bool {
        let Some(source) = self.asked.remove(&id) else { return false };
        let why = match kind {
            FailKind::TooLarge => Why::TooLarge,
            FailKind::Api { code } if code == "not_an_image" => Why::Unreadable,
            _ => Why::NotLoaded,
        };
        let again = (kind.retryable() || *kind == FailKind::CapReached).then(|| now + RETRY_AFTER);
        self.entries.insert(source, Entry::Failed { why, again });
        true
    }

    /// The picture `source` was decoded (or refused): it is sent to the terminal before the next
    /// frame, and the least recently drawn go while there are too many.
    pub fn decoded(&mut self, source: Source, result: Result<Decoded, Refused>) {
        if self.entries.get(&source) != Some(&Entry::Decoding) {
            return;
        }
        let d = match result {
            Ok(d) => d,
            Err(refused) => {
                let why = match refused {
                    Refused::TooLarge => Why::TooLarge,
                    Refused::Unreadable => Why::Unreadable,
                };
                self.entries.insert(source, Entry::Failed { why, again: None });
                return;
            }
        };
        self.last_id = if self.last_id >= graphics::ID_MAX { 1 } else { self.last_id + 1 };
        let id = self.last_id;
        let (cols, rows) = (d.cols.min(graphics::MARKS), d.rows.min(graphics::MARKS));
        self.graphics.push_str(&graphics::transmit(id, cols, rows, &d.png));
        self.bytes += d.png.len();
        self.entries.insert(source.clone(), Entry::Ready { id, cols, rows, bytes: d.png.len() });
        self.held.push_back(source);
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
}

#[cfg(test)]
mod tests;
