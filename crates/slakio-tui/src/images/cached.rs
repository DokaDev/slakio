//! The pictures kept on disk between runs ([`slakio_core::picture_cache`]), as the UI uses them:
//! profile photos and a workspace's own emoji, never a file's preview (a copy of a file of the
//! signed-in user's workspace, which another account on the same machine must not be shown).
//! A picture a frame wants and does not have is looked for there first: the look is a job like
//! a decode ([`Job::from_cache`], counted among the pictures on their way, so the same bounds on
//! what is asked hold), which the binary runs off the loop's thread with [`run`]. Found, it is
//! decoded within the same bounds as a fetched picture (what is on disk is not trusted: one
//! that does not decode is removed); not found, the next frame that still wants it asks the
//! backend ([`Images::ask`]), and the picture made from the answer is kept for the next time.
//! A picture parked or removed from the terminal comes back from there, not from the network.
//! Nothing here reads or writes the disk on the loop's thread.

use super::decode::{self, Refused, Shape};
use super::{Entry, Images, Job, Key, Prepared, note};
use slakio_core::backend::Generation;
use slakio_core::diag::{Diag, Kind, Stage};
use slakio_core::images::Source;
use slakio_core::picture_cache::{Loaded, PictureCache, Stored, Swept};

/// The cells a picture fills, as part of its key on disk.
pub fn shape_tag(shape: Shape) -> String {
    match shape {
        Shape::Preview => "preview".to_string(),
        Shape::Emoji => "emoji".to_string(),
        Shape::Photo { cols, rows } => format!("photo-{cols}x{rows}"),
    }
}

/// `job` made ready, for cells of `cell` pixels, as the binary runs it (a blocking task): with
/// a cache, a look there ([`Refused::NotCached`] when nothing there can be shown), or the bytes
/// fetched decoded and the picture made of them kept. Blocking work: never on the loop's thread.
pub fn run(
    job: &Job,
    cell: Option<(u16, u16)>,
    cache: Option<&PictureCache>,
    diag: &Diag,
) -> Result<Prepared, Refused> {
    // A file's preview is never kept: fetched each run, with the session's cookie.
    let Some(cache) = cache.filter(|_| kept(&job.source)) else {
        return if job.from_cache { Err(Refused::NotCached) } else { super::prepare(job, cell) };
    };
    let key = (job.source.clone(), job.shape);
    let tag = shape_tag(job.shape);
    if !job.from_cache {
        let d = decode::decode(&job.bytes, job.shape, cell)?;
        match cache.store(&job.source, &tag, &d.png) {
            Stored::Stored(swept) => {
                note::line(diag, &key, Stage::CacheStored, "ok", &[("bytes", d.png.len().to_string())]);
                swept.into_iter().for_each(|s| swept_line(diag, s));
            }
            Stored::Refused(why) => note::line(diag, &key, Stage::CacheRefused, why, &[]),
        }
        return Ok(super::made(job, &d));
    }
    match cache.load(&job.source, &tag) {
        Loaded::Hit(png) => match decode::decode(&png, job.shape, cell) {
            Ok(d) => {
                note::line(diag, &key, Stage::CacheHit, "ok", &[]);
                Ok(super::made(job, &d))
            }
            Err(_) => {
                cache.forget(&job.source, &tag);
                note::line(diag, &key, Stage::CacheRefused, "decode", &[]);
                Err(Refused::NotCached)
            }
        },
        Loaded::Miss => {
            note::line(diag, &key, Stage::CacheMiss, "absent", &[]);
            Err(Refused::NotCached)
        }
        Loaded::Expired => {
            note::line(diag, &key, Stage::CacheMiss, "expired", &[]);
            Err(Refused::NotCached)
        }
        Loaded::Refused(why) => {
            note::line(diag, &key, Stage::CacheRefused, why, &[]);
            Err(Refused::NotCached)
        }
    }
}

/// At the start, off the loop's thread: the folder made or found ([`PictureCache::open`]; only
/// then are pictures kept), handed to `made` when it was made now (to keep it out of backups),
/// and swept ([`PictureCache::sweep`]).
pub fn start(cache: &PictureCache, diag: &Diag, made: impl FnOnce(&std::path::Path)) {
    match cache.open() {
        None => return diag.line(Kind::PictureCache, Stage::CacheRefused, "folder", &[]),
        Some(true) => made(cache.dir()),
        Some(false) => {}
    }
    sweep(cache, diag);
}

/// The sweep ([`PictureCache::sweep`]), off the loop's thread.
pub fn sweep(cache: &PictureCache, diag: &Diag) {
    if let Some(s) = cache.sweep() {
        swept_line(diag, s);
    }
}

fn swept_line(diag: &Diag, s: Swept) {
    let (n, kept, bytes) = (s.removed.to_string(), s.kept.to_string(), s.bytes.to_string());
    diag.line(Kind::PictureCache, Stage::CacheSwept, "ok", &[("n", &n), ("kept", &kept), ("bytes", &bytes)]);
}

/// Pictures of `source`'s kind are kept on disk: photos and emoji, never a file's preview.
fn kept(source: &Source) -> bool {
    !matches!(source, Source::Thumb(_))
}

impl Images {
    /// `key` is looked for on disk before it is asked for: there is a cache and it keeps the kind.
    pub(super) fn on_disk(&self, key: &Key) -> bool {
        self.cache.is_some() && kept(&key.0)
    }

    /// The picture diagnostics (the binary's jobs write to them too).
    pub fn diag(&self) -> &Diag {
        &self.diag
    }

    /// The looks in the cache asked since the last call, for the binary to run ([`run`]).
    pub fn take_lookups(&mut self) -> Vec<Job> {
        std::mem::take(&mut self.lookups)
    }

    /// `key` is looked for in the cache: on its way until the look ends.
    pub(super) fn look_up(&mut self, key: Key) {
        let id = self.next_id();
        // Never a backend's request id (those count up from one), and never two at once.
        let request = Generation(u64::MAX - u64::from(id));
        self.entries.insert(key.clone(), Entry::Decoding(request));
        let (source, shape) = key;
        self.lookups.push(Job { source, request, id, bytes: Vec::new(), shape, from_cache: true });
    }

    /// `key` is not in the cache: the next frame that wants it asks the backend.
    pub(super) fn not_cached(&mut self, key: Key) {
        self.entries.remove(&key);
        if self.missed.len() >= super::FAILED_MAX {
            self.missed.clear();
        }
        self.missed.insert(key);
    }
}

#[cfg(test)]
mod tests;
