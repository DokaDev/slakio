//! The terminal's room as the pictures on screen fill it: how many they are and their bytes
//! ([`ENTRIES_MAX`], [`BYTES_CAP`]). Nothing more is asked for while they fill it, and a parked
//! picture stays parked, whatever the screen, until it would fit.

use super::{BYTES_CAP, CHIP_PHOTO, ENTRIES_MAX, Entry, Images, Key};

impl Images {
    /// The pictures held that stay on screen for `key` (a list photo yields its room to the
    /// timeline's pictures, never the other way): how many, and their bytes.
    pub(super) fn staying(&self, key: &Key) -> (usize, usize) {
        let chip = key.1 == CHIP_PHOTO;
        let staying = self.held.iter().filter(|k| self.on_screen(k) && (chip || k.1 != CHIP_PHOTO));
        staying.fold((0, 0), |(n, bytes), k| match self.entries.get(k) {
            Some(Entry::Ready { bytes: b, .. }) => (n + 1, bytes + b),
            _ => (n + 1, bytes),
        })
    }

    /// No room for `key` beside the pictures staying on screen and the `in_flight` ones on their
    /// way: their count reaches [`ENTRIES_MAX`], or their bytes, those on their way and `key`
    /// counted as large as the staying ones on average, pass [`BYTES_CAP`].
    pub(super) fn full(&self, key: &Key, in_flight: usize) -> bool {
        let (n, bytes) = self.staying(key);
        let average = bytes.checked_div(n).unwrap_or(0);
        n + in_flight >= ENTRIES_MAX || bytes + (in_flight + 1) * average > BYTES_CAP
    }

    /// A parked picture that would now fit beside those staying on screen is forgotten, to be
    /// asked again; the others stay parked (not fetched again for nothing).
    pub(super) fn unpark(&mut self) {
        let parked: Vec<(Key, usize)> = self
            .entries
            .iter()
            .filter_map(|(k, e)| match e {
                Entry::Parked { bytes } => Some((k.clone(), *bytes)),
                _ => None,
            })
            .collect();
        for (key, bytes) in parked {
            let (n, staying) = self.staying(&key);
            if n < ENTRIES_MAX && staying + bytes <= BYTES_CAP {
                self.entries.remove(&key);
            }
        }
    }
}
