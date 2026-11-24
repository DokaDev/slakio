//! Pictures that move (a GIF emoji or photo, [`super::frames`]), as the app drives them: the
//! terminal is sent the next frame under the picture's own id when it is due, so the placeholder
//! cells on screen stay as they are. The kitty protocol deletes an image and its placements
//! when data is sent again under its id: each frame is sent with `a=T,U=1` as the first one is,
//! which places it virtually again in the same command. Frames go as direct data (`t=d`);
//! the protocol's own animation commands are never sent.
//!
//! Only what the latest shown frame showed moves, at most [`MOVING_MAX`] pictures (the first
//! ones it drew); each frame is shown at least [`super::frames::DELAY_MIN`]. The times frames
//! are due fall on a grid of [`GRID`], so pictures moving together wake the loop together. The
//! loop wakes for a frame only while a picture moves ([`Images::frame_at`]). `animate = "off"`
//! ([`Images::set_animate`]) puts every picture back on its first frame and nothing moves; with
//! pictures off nothing is held, so nothing moves either.

use super::frames::Moving;
use super::{Entry, Images, Key, graphics};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The most pictures moving at once.
pub const MOVING_MAX: usize = 8;
/// The times frames are due are whole steps of this from the first.
pub const GRID: Duration = Duration::from_millis(50);

/// A moving picture's frames as sent (the commands, how long each is shown), how many frames
/// were read and why it stays still (none of them: it does not move).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sent {
    pub frames: Vec<(String, Duration)>,
    pub seen: usize,
    pub why: &'static str,
}

/// `m` made into the commands that send each frame as picture `id`, `cols` × `rows` cells.
pub(super) fn sent(id: u32, cols: u16, rows: u16, m: &Moving) -> Sent {
    let frames = if m.moves() {
        m.frames.iter().map(|f| (graphics::transmit(id, cols, rows, &f.png), f.delay)).collect()
    } else {
        Vec::new()
    };
    Sent { frames, seen: m.seen, why: m.why }
}

#[derive(Debug)]
struct Anim {
    id: u32,
    frames: Vec<(String, Duration)>,
    /// The frame the terminal shows.
    at: usize,
    /// When the next is due; `None` while it does not move.
    next: Option<Instant>,
}

/// The pictures that can move.
#[derive(Debug, Default)]
pub(super) struct Anims {
    /// `animate = "off"`.
    still: bool,
    list: HashMap<Key, Anim>,
    /// Those the frame being noted showed, in its order (at most [`MOVING_MAX`]).
    seen: Vec<Key>,
    /// Those the latest shown frame showed: the ones that move.
    moving: Vec<Key>,
    /// The start of the grid.
    epoch: Option<Instant>,
}

impl Anims {
    /// Picture `key`, sent as `id`, has these frames (it moves when it has more than one).
    pub(super) fn add(&mut self, key: &Key, id: u32, frames: Vec<(String, Duration)>) {
        self.moving.retain(|k| k != key);
        if frames.len() > 1 {
            self.list.insert(key.clone(), Anim { id, frames, at: 0, next: None });
        } else {
            self.list.remove(key);
        }
    }

    /// The frame being noted showed `key`.
    pub(super) fn saw(&mut self, key: &Key) {
        if !self.still && self.seen.len() < MOVING_MAX && self.list.contains_key(key) && !self.seen.contains(key) {
            self.seen.push(key.clone());
        }
    }

    /// `at` moved up to the grid.
    fn on_grid(&mut self, at: Instant) -> Instant {
        let epoch = *self.epoch.get_or_insert(at);
        let since = at.saturating_duration_since(epoch).as_nanos();
        let step = GRID.as_nanos();
        let steps = u64::try_from(since.div_ceil(step)).unwrap_or(u64::MAX);
        epoch + GRID.saturating_mul(u32::try_from(steps).unwrap_or(u32::MAX))
    }
}

/// `key` is held by the terminal as picture `id` (not removed, nor sent again as another).
fn held(entries: &HashMap<Key, Entry>, key: &Key, id: u32) -> bool {
    matches!(entries.get(key), Some(Entry::Ready { id: i, .. }) if *i == id)
}

impl Images {
    /// After a frame (at `now`): what it showed moves from now on, what it did not stops where it
    /// is; a picture no longer held (or held as another id) is forgotten.
    pub(super) fn moving(&mut self, now: Instant) {
        let entries = &self.entries;
        self.anims.list.retain(|k, a| held(entries, k, a.id));
        let seen = std::mem::take(&mut self.anims.seen);
        for key in &self.anims.moving {
            if let Some(a) = self.anims.list.get_mut(key).filter(|_| !seen.contains(key)) {
                a.next = None;
            }
        }
        for key in &seen {
            let due = match self.anims.list.get(key) {
                Some(a) if a.next.is_none() => now + a.frames[a.at].1,
                _ => continue,
            };
            let due = self.anims.on_grid(due);
            if let Some(a) = self.anims.list.get_mut(key) {
                a.next = Some(due);
            }
        }
        self.anims.moving = seen;
    }

    /// When the next frame of a moving picture is due, if one moves.
    pub fn frame_at(&self) -> Option<Instant> {
        if !self.on || self.anims.still {
            return None;
        }
        self.anims.moving.iter().filter_map(|k| self.anims.list.get(k)?.next).min()
    }

    /// The loop woke at `now`: each moving picture whose next frame is due is sent it (written
    /// before the next frame is drawn). `true` when one was.
    pub fn advance(&mut self, now: Instant) -> bool {
        if !self.on || self.anims.still {
            return false;
        }
        let mut sent = false;
        for key in self.anims.moving.clone() {
            let Some(a) = self.anims.list.get_mut(&key) else { continue };
            if a.next.is_none_or(|at| at > now) || !held(&self.entries, &key, a.id) {
                continue;
            }
            a.at = (a.at + 1) % a.frames.len();
            self.graphics.push_str(&a.frames[a.at].0);
            let due = now + a.frames[a.at].1;
            let due = self.anims.on_grid(due);
            if let Some(a) = self.anims.list.get_mut(&key) {
                a.next = Some(due);
            }
            sent = true;
        }
        sent
    }

    /// `animate` on or off (`on`: the default). Off, every picture not on its first frame is sent
    /// it again, and nothing moves until it is on again.
    pub fn set_animate(&mut self, on: bool) {
        self.anims.still = !on;
        if on {
            return;
        }
        self.anims.moving.clear();
        self.anims.seen.clear();
        for (key, a) in &mut self.anims.list {
            a.next = None;
            if a.at != 0 {
                a.at = 0;
                if self.on && held(&self.entries, key, a.id) {
                    self.graphics.push_str(&a.frames[0].0);
                }
            }
        }
    }

    /// `animate` is on.
    pub fn animates(&self) -> bool {
        !self.anims.still
    }
}

#[cfg(test)]
mod tests;
