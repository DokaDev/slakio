//! Pictures in the timeline: previews of the pictures messages share, under their cards,
//! people's profile photos in the comfortable layout's picture slot and in the list's DM rows
//! (`avatars = "image"`), and a workspace's own emoji in text and reaction chips ([`custom`],
//! two cells each, [`Shape::Emoji`]), drawn with the kitty graphics protocol where the terminal draws it
//! ([`detect`]); elsewhere, and while a picture is not there yet, the text card and the initials
//! stay as they are. A picture is kept per address and the cells it fills ([`Shape`]): a photo
//! shown in both places is two pictures to the terminal, fetched once (the bytes of the last
//! [`FETCHED_MAX`] photos fetched are kept, and its other place is decoded from them; while one
//! place's is on its way, the other's waits for it).
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
//! (and fetched again if it is drawn again), but never one the latest frame or the one before
//! it looked at: when every picture held is on screen, a list photo makes room for a timeline
//! picture, and otherwise the new picture is not sent and waits, parked, until it fits beside
//! those on screen. While the pictures on screen (their count or their bytes) fill the
//! terminal's room, nothing more is asked for ([`room`]). A frame draws at most
//! [`EMOJI_PER_FRAME`] distinct emoji pictures ([`Images::look_emoji`]), counting only what it
//! shows ([`Images::not_shown`]); the rest stay text. The timeline's pictures are asked for
//! before the list's photos, which yield their room to them. A picture that failed says why on
//! its card (too large, unreadable, not loaded); one that failed for the network or the request
//! budget is asked again after [`RETRY_AFTER`] ([`RETRY_SOON`] for a busy or offline backend),
//! when it is drawn again; at most [`FAILED_MAX`] failures are kept. With a cache on disk
//! ([`cached`]), a profile photo or an emoji is looked for there before it is asked for, at
//! once and outside the network's [`IN_FLIGHT_MAX`] (a file's preview never is: it is never
//! kept on disk). A GIF emoji or photo moves ([`frames`], [`animate`]).

mod ahead;
pub mod animate;
mod ask;
pub mod cached;
pub mod custom;
pub mod decode;
pub mod detect;
pub mod frames;
pub mod graphics;
mod note;
mod room;
mod wake;

pub use ahead::{AHEAD_MAX, AHEAD_PER_HOUR, AHEAD_PER_MINUTE, AHEAD_QUEUE_MAX};
use decode::{Refused, Shape};
use slakio_core::backend::Generation;
use slakio_core::diag::{Diag, Stage};
use slakio_core::images::Source;
use slakio_core::model::WorkspaceId;
use slakio_core::picture_cache::PictureCache;
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
/// The most photos whose fetched bytes are kept, for their other place ([`Images::ask`]).
pub const FETCHED_MAX: usize = 64;
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
    /// Looked for in the cache on disk, as request `.0` of its own (not the network's share).
    Looking(Generation),
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
    /// Fetched while every picture held was on screen: not sent, and not asked again until its
    /// `bytes` would fit beside those on screen ([`room`]; drawn as not here yet meanwhile).
    Parked {
        bytes: usize,
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
    /// No bytes: the picture is looked for in the cache on disk ([`cached::run`]).
    pub from_cache: bool,
    /// No picture: every picture kept on disk is removed ([`Images::forget_kept`]).
    pub empty: bool,
    /// The cache's count of emptyings when the job was made: emptied since, nothing is kept.
    pub epoch: u64,
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
    /// Its frames, when it moves ([`animate`]; `bytes` counts them all).
    pub moving: animate::Sent,
}

/// `job` decoded ([`decode::decode`], for cells of `cell` pixels) and made into the commands
/// that send it. Blocking work: never on the loop's thread.
pub fn prepare(job: &Job, cell: Option<(u16, u16)>) -> Result<Prepared, Refused> {
    decode::decode(&job.bytes, job.shape, cell).map(|d| made(job, &d))
}

/// `d`, decoded for `job`, made into the commands that send it.
fn made(job: &Job, d: &decode::Decoded) -> Prepared {
    let (cols, rows) = (d.cols.min(graphics::MARKS), d.rows.min(graphics::MARKS));
    let (transmit, moving) =
        (graphics::transmit(job.id, cols, rows, &d.png), animate::sent(job.id, cols, rows, &d.moving));
    Prepared { id: job.id, cols, rows, bytes: d.png.len().max(d.moving.bytes()), transmit, from: d.from, moving }
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
    /// The pictures kept on disk between runs (none in the demo or with `cache = "off"`).
    pub cache: Option<PictureCache>,
    /// Looked for there and not found: asked of the backend when next wanted.
    missed: HashSet<Key>,
    /// Looks there to hand to the binary ([`Images::take_lookups`]).
    lookups: Vec<Job>,
    anims: animate::Anims,
    /// The DM peers' photos asked for ahead of their rows ([`ahead`]).
    ahead: ahead::Ahead,
    /// The bytes of the last [`FETCHED_MAX`] photos fetched, the oldest first.
    fetched: VecDeque<(Source, Vec<u8>)>,
}

impl Images {
    /// Write the picture diagnostics to `diag` (the emoji lists' too).
    pub fn set_diag(&mut self, diag: Diag) {
        self.customs.diag = diag.clone();
        self.diag = diag;
    }

    /// [`Images::on`] from what decides it ([`detect::images_on`]). Turned off, every picture
    /// leaves the terminal at once (each by its id, then all) and nothing asked or looked for is
    /// waited for.
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
            self.anims.forget();
            // Looks not handed over yet are not run; the emptying of the disk always is.
            self.lookups.retain(|j| j.empty);
            self.missed.clear();
            self.forget_ahead();
            self.fetched.clear();
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
        self.forget_ahead();
    }

    /// Photo `address` is no one's any more: it leaves the terminal (in each of its shapes), and
    /// is not waited for if asked for.
    pub fn forget_photo(&mut self, address: &str) {
        self.forget_where(|k| matches!(&k.0, Source::Avatar(a) if a == address));
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
        self.fetched.retain(|(s, _)| !gone(&(s.clone(), PHOTO)));
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
            Some(Entry::Asked(_) | Entry::Decoding(_) | Entry::Looking(_)) => Look::Pending,
            Some(Entry::Parked { .. }) | None => {
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

    /// The id of the next picture sent.
    fn next_id(&mut self) -> u32 {
        self.last_id = if self.last_id >= graphics::ID_MAX { 1 } else { self.last_id + 1 };
        self.last_id
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

    /// The picture of `job` was made ready (or refused): it is sent to the terminal before the
    /// next frame, and the least recently drawn go while there are too many, never one on
    /// screen but a list photo for a timeline picture; with no room, it is parked. A result for
    /// a request no longer waited for changes nothing.
    pub fn decoded(&mut self, job: (Source, Generation), result: Result<Prepared, Refused>) {
        let (source, request) = job;
        // Request ids are never used twice: the one decoding it is the picture.
        let mine = |e: &Entry| *e == Entry::Decoding(request) || *e == Entry::Looking(request);
        let Some(key) = self.entries.iter().find(|(k, e)| k.0 == source && mine(e)).map(|(k, _)| k.clone()) else {
            return;
        };
        let ahead = self.ahead.flying.remove(&key);
        let mut p = match result {
            Ok(p) => p,
            Err(Refused::NotCached) => return self.not_cached(key),
            Err(refused) => {
                let why = if refused == Refused::TooLarge { Why::TooLarge } else { Why::Unreadable };
                let result = if why == Why::TooLarge { "decode-too-large" } else { "decode-unreadable" };
                note::line(&self.diag, &key, Stage::Refused, result, &[]);
                return self.fail(key, why, None);
            }
        };
        note::decoded(&self.diag, &key, &p);
        if ahead && self.let_go_ahead(&key, p.bytes) {
            return;
        }
        // A photo ahead not on screen is the least recently drawn: the first to go.
        let unseen = ahead && !self.seen_now(&key);
        while self.held.len() >= ENTRIES_MAX || (self.bytes + p.bytes > BYTES_CAP && !self.held.is_empty()) {
            let off = self.held.iter().position(|k| !self.on_screen(k));
            let chip = || self.held.iter().position(|k| k.1 == CHIP_PHOTO);
            let Some(i) = off.or_else(|| (key.1 != CHIP_PHOTO).then(chip).flatten()) else {
                // Never sent, so nothing to remove from the terminal.
                let held = [("held", self.held.len().to_string()), ("bytes", self.bytes.to_string())];
                note::line(&self.diag, &key, Stage::Parked, "room", &held);
                self.entries.insert(key, Entry::Parked { bytes: p.bytes });
                return;
            };
            let old = self.held.remove(i).expect("found");
            if let Some(Entry::Ready { id, bytes, .. }) = self.entries.remove(&old) {
                self.bytes -= bytes;
                self.graphics.push_str(&graphics::delete(id));
            }
        }
        self.anims.add(&key, p.id, std::mem::take(&mut p.moving.frames));
        self.graphics.push_str(&p.transmit);
        self.highest = self.highest.max(p.id);
        self.bytes += p.bytes;
        self.entries.insert(key.clone(), Entry::Ready { id: p.id, cols: p.cols, rows: p.rows, bytes: p.bytes });
        if unseen { self.held.push_front(key) } else { self.held.push_back(key) }
    }

    /// Each of `ended` ([`Images::decoded`]), in order.
    pub fn decoded_all(&mut self, ended: Vec<((Source, Generation), Result<Prepared, Refused>)>) {
        ended.into_iter().for_each(|(job, result)| self.decoded(job, result));
    }

    /// The commands for the terminal not written yet (sending and removing pictures).
    pub fn take_graphics(&mut self) -> String {
        std::mem::take(&mut self.graphics)
    }
}

#[cfg(test)]
mod tests;
