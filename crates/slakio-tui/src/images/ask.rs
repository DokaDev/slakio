//! Asking for the pictures a frame wanted ([`Images::ask`]), and what the backend answered
//! ([`Images::answered`], [`Images::failed`]).

use super::cached::LOOKUPS_MAX;
use super::{CHIP_PHOTO, Entry, IN_FLIGHT_MAX, Images, Job, Key, RETRY_AFTER, RETRY_SOON, Why, note};
use crate::app::requests::Requests;
use slakio_core::backend::{Command, FailKind, Generation};
use slakio_core::diag::Stage;
use slakio_core::images::BYTES_MAX;
use slakio_core::model::WorkspaceId;
use slakio_core::picture_cache::PictureCache;
use std::collections::HashSet;
use std::time::Instant;

impl Images {
    /// The pictures asked of the backend or being decoded from its answer (the network's share).
    pub(super) fn in_flight(&self) -> usize {
        self.entries.values().filter(|e| matches!(e, Entry::Asked(_) | Entry::Decoding(_))).count()
    }

    /// The looks in the cache on disk not ended yet.
    pub(super) fn looking(&self) -> usize {
        self.entries.values().filter(|e| matches!(e, Entry::Looking(_))).count()
    }

    /// Each picture of `wanted` the cache on disk may hold, not there yet (or failed, its wait
    /// over) and not missed there since, is looked for there now: no wait for the network's
    /// share or the room (a picture found that does not fit is parked), at most [`LOOKUPS_MAX`]
    /// looks at once. `true` when one was.
    fn look_up_all(&mut self, wanted: &[(WorkspaceId, Key, bool)], now: Instant) -> bool {
        let (mut looking, mut any) = (self.looking(), false);
        for (_, key, _) in wanted {
            if looking >= LOOKUPS_MAX {
                break;
            }
            if !self.on_disk(key) || self.missed.contains(key) {
                continue;
            }
            match self.entries.get(key) {
                None => {}
                Some(Entry::Failed { again: Some(at), .. }) if *at <= now => self.failed.retain(|k| k != key),
                Some(_) => continue,
            }
            self.look_up(key.clone());
            (looking, any) = (looking + 1, true);
        }
        any
    }

    /// After a frame: what it showed is the most recently drawn, and what it would have shown
    /// is asked for, the timeline's pictures before the list's photos: first everything the
    /// cache on disk may hold is looked for there ([`Images::look_up_all`]), then the rest is
    /// asked of the backend (at most [`IN_FLIGHT_MAX`] at once, and none while the pictures on
    /// screen and those on their way fill the terminal's room; a failed one once its wait is
    /// over); what it only laid out is neither. A parked picture that would now fit may be
    /// asked again. Then, everything wanted asked for, a photo ahead ([`super::ahead`]). `true`
    /// when something was asked or looked for.
    pub fn ask(&mut self, requests: &mut Requests, now: Instant) -> bool {
        let mut screen = HashSet::new();
        for (key, _) in self.drawn.take().into_iter().filter(|d| d.1) {
            if let Some(i) = self.held.iter().position(|k| *k == key) {
                let s = self.held.remove(i).expect("found");
                self.held.push_back(s);
            }
            self.anims.saw(&key);
            screen.insert(key);
        }
        self.moving(now);
        self.last_screen = std::mem::replace(&mut self.screen, screen);
        self.unpark();
        self.emoji_frame.take();
        let mut wanted: Vec<_> = self.wanted.take().into_iter().filter(|w| w.2).collect();
        wanted.sort_by_key(|w| w.1.1 == CHIP_PHOTO);
        let mut asked = self.look_up_all(&wanted, now);
        let mut all = true;
        for (ws, key, _) in wanted {
            let in_flight = self.in_flight();
            if in_flight >= IN_FLIGHT_MAX {
                if self.diag.on() {
                    note::once(&self.diag, &key, Stage::Skipped, "in-flight", &[("in_flight", in_flight.to_string())]);
                }
                all = false;
                break;
            }
            if self.full(&key, in_flight + self.looking()) {
                if self.diag.on() {
                    let staying = self.staying(&key).0.to_string();
                    note::once(&self.diag, &key, Stage::Skipped, "room", &[("staying", staying)]);
                }
                continue;
            }
            match self.entries.get(&key) {
                None => {}
                Some(Entry::Failed { again: Some(at), .. }) if *at <= now => {}
                Some(_) => continue,
            }
            // Looked for on disk first (next frame, past [`LOOKUPS_MAX`]); not found there, asked.
            if self.on_disk(&key) && !self.missed.remove(&key) {
                continue;
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
        // Photos ahead only once everything this frame wanted is asked for.
        if all {
            asked |= self.ask_ahead(requests, now);
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
        let (picture, epoch) = (self.next_id(), self.cache.as_ref().map_or(0, PictureCache::epoch));
        Some(Job { source, request: id, id: picture, bytes, shape, from_cache: false, empty: false, epoch })
    }

    /// Picture request `id` failed (`kind`) at `now`: `true` when it was one of these.
    pub fn failed(&mut self, id: Generation, kind: &FailKind, now: Instant) -> bool {
        let Some(key) = self.asked.remove(&id) else { return false };
        self.ahead.flying.remove(&key);
        // Busy, offline or the budget spent, for any picture: nothing more is asked ahead a while.
        if matches!(kind, FailKind::Busy | FailKind::Offline | FailKind::CapReached) {
            self.ahead.paused_until = Some(now + RETRY_AFTER);
        }
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
}
