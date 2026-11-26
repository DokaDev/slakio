//! The terminal data moving pictures may send: at most [`ANIM_BYTES_PER_SECOND`] over any
//! second. A frame that does not fit is skipped (the terminal keeps the frame it shows), so a
//! busy screen or a slow link loses frames, never speed.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// The most bytes of frame commands sent in any one second.
pub const ANIM_BYTES_PER_SECOND: usize = 1 << 20;
const WINDOW: Duration = Duration::from_secs(1);

/// The frame commands sent in the last second: when, and how many bytes.
#[derive(Debug, Default)]
pub(super) struct Spent {
    sent: VecDeque<(Instant, usize)>,
    total: usize,
}

impl Spent {
    /// The bytes that may still be sent at `now`.
    pub(super) fn room(&mut self, now: Instant) -> usize {
        while let Some(&(at, n)) = self.sent.front() {
            if at + WINDOW > now {
                break;
            }
            self.sent.pop_front();
            self.total -= n;
        }
        ANIM_BYTES_PER_SECOND.saturating_sub(self.total)
    }

    /// A frame of `n` bytes fits at `now`: within the room, or the only one in the second (one
    /// larger than the whole budget still moves, slowly).
    pub(super) fn fits(&mut self, now: Instant, n: usize) -> bool {
        self.room(now);
        fits(n, self.total)
    }

    /// When a frame of `n` bytes will fit if nothing more is sent; `None`: it fits now.
    pub(super) fn free_at(&self, n: usize) -> Option<Instant> {
        let mut total = self.total;
        if fits(n, total) {
            return None;
        }
        for &(at, k) in &self.sent {
            total -= k;
            if fits(n, total) {
                return Some(at + WINDOW);
            }
        }
        None
    }

    /// `n` bytes were sent at `now`.
    pub(super) fn spend(&mut self, now: Instant, n: usize) {
        self.sent.push_back((now, n));
        self.total += n;
    }
}

/// `n` bytes fit beside `total` sent in the second: within the budget, or alone.
fn fits(n: usize, total: usize) -> bool {
    total == 0 || n <= ANIM_BYTES_PER_SECOND.saturating_sub(total)
}
