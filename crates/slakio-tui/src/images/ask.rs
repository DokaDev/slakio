//! Asking for the pictures a frame wanted ([`Images::ask`]), and what the backend answered
//! ([`Images::answered`], [`Images::failed`]).

use super::{CHIP_PHOTO, Entry, IN_FLIGHT_MAX, Images, Job, RETRY_AFTER, RETRY_SOON, Why, note};
use crate::app::requests::Requests;
use slakio_core::backend::{Command, FailKind, Generation};
use slakio_core::diag::Stage;
use slakio_core::images::BYTES_MAX;
use slakio_core::picture_cache::PictureCache;
use std::collections::HashSet;
use std::time::Instant;

impl Images {
    fn in_flight(&self) -> usize {
        self.entries.values().filter(|e| matches!(e, Entry::Asked(_) | Entry::Decoding(_))).count()
    }

    /// After a frame: what it showed is the most recently drawn, and what it would have shown
    /// is asked for, the timeline's pictures before the list's photos (at most [`IN_FLIGHT_MAX`]
    /// at once, and none while the pictures on screen and those on their way fill the
    /// terminal's room; a failed one once its wait is over); what it only laid out is neither.
    /// A parked picture that would now fit may be asked again. `true` when something was asked.
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
            if self.full(&key, in_flight) {
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
            if self.on_disk(&key) && !self.entries.contains_key(&key) && !self.missed.remove(&key) {
                self.look_up(key);
                asked = true;
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
