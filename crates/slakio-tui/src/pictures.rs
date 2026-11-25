//! The binary's part of the pictures ([`slakio_tui::images`]): making them ready off the loop's
//! thread (a blocking task each, decode and the commands that send them; the app asks for at
//! most a few at once, and a look in the cache on disk waits for one of
//! [`cached::DISK_LOADERS`] loaders) and writing those commands before a frame is drawn, the
//! only part left on the loop's thread.
//!
//! With the local cache on (not in the demo), the pictures made are kept on disk and looked for
//! there first ([`slakio_tui::images::cached`]); the folder is made (when pictures are on at
//! the start: only then are they kept in this run) and swept once at the start, on a blocking
//! task too.
//!
//! A panic in the decoder is caught there ([`slakio_tui::terminal::catching`]: the panic hook
//! leaves the terminal as it is): the picture is not shown (one read from the disk is removed
//! there), and the error log says so in one line that names no address. One after the picture
//! was handed over (while it is kept on disk) leaves it shown, and the error log says that.

use ratatui::crossterm::terminal::window_size;
use slakio_core::backend::Generation;
use slakio_core::diag::Diag;
use slakio_core::fault::{ErrorLog, Fault};
use slakio_core::images::Source;
use slakio_core::picture_cache::PictureCache;
use slakio_tui::app::App;
use slakio_tui::images::decode::Refused;
use slakio_tui::images::{Images, Job, Prepared, cached};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Semaphore, mpsc};

/// A job's picture and request, and what came of it.
type Done = ((Source, Generation), Result<Prepared, Refused>);

/// The decodes on their way, and their results.
pub(crate) struct Pictures {
    tx: mpsc::UnboundedSender<(Done, bool)>,
    rx: mpsc::UnboundedReceiver<(Done, bool)>,
    /// The error log.
    errors: Option<PathBuf>,
    /// The pictures kept on disk, and the diagnostics the jobs write to.
    cache: Option<PictureCache>,
    diag: Diag,
    /// Debug builds only (`SLAKIO_DEBUG_DECODE_PANIC=1`): the decoder panics, for the test that
    /// a panic there leaves the terminal as it is; `=keep`: the job panics once the picture is
    /// handed over, as keeping it on disk might.
    panic: bool,
    panic_keep: bool,
    /// The looks in the cache on disk that may run at once ([`cached::DISK_LOADERS`]).
    loaders: Arc<Semaphore>,
}

/// A cell's size in pixels, when the terminal says the window's (an `ioctl`, not a question on
/// standard input).
fn cell() -> Option<(u16, u16)> {
    let w = window_size().ok()?;
    (w.columns > 0 && w.rows > 0 && w.width > 0 && w.height > 0).then(|| (w.width / w.columns, w.height / w.rows))
}

impl Pictures {
    /// The decodes of `images`, which from now on look in the cache on disk when `local` keeps
    /// one and this is not the demo (swept now, off the loop's thread).
    pub(crate) fn new(errors: Option<PathBuf>, images: &mut Images, local: &crate::store::Local) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let asked = std::env::var_os("SLAKIO_DEBUG_DECODE_PANIC").filter(|_| cfg!(debug_assertions));
        let (panic, panic_keep) = (asked.as_ref().is_some_and(|v| v == "1"), asked.is_some_and(|v| v == "keep"));
        images.cache = local.pictures().filter(|_| !images.demo);
        let (cache, diag) = (images.cache.clone(), images.diag().clone());
        if let Some(c) = cache.clone().filter(|_| images.on) {
            let (d, made) = (diag.clone(), local.exclude_pictures());
            tokio::task::spawn_blocking(move || cached::start(&c, &d, made));
        }
        let loaders = Arc::new(Semaphore::new(cached::DISK_LOADERS));
        Self { tx, rx, errors, cache, diag, panic, panic_keep, loaders }
    }

    /// Make `job` ready on a blocking task; its result comes from [`Pictures::next`].
    pub(crate) fn start(&self, job: Job) {
        let (tx, cell, panic, panic_keep) = (self.tx.clone(), cell(), self.panic, self.panic_keep);
        let (cache, diag, errors) = (self.cache.clone(), self.diag.clone(), self.errors.clone());
        let disk = job.from_cache;
        blocking(&self.loaders, disk, move || {
            // The cache holds nothing a panic part way leaves broken (a write is whole or not
            // at all). The picture goes to the UI before it is kept on disk.
            let sent = std::cell::Cell::new(false);
            let made = slakio_tui::terminal::catching(std::panic::AssertUnwindSafe(|| {
                assert!(!panic, "SLAKIO_DEBUG_DECODE_PANIC: a panic in the decoder");
                cached::run_then_keep(&job, cell, cache.as_ref(), &diag, |result| {
                    let _ = tx.send((((job.source.clone(), job.request), result), false));
                    sent.set(true);
                    assert!(!panic_keep, "SLAKIO_DEBUG_DECODE_PANIC=keep: a panic while it is kept");
                });
            }));
            // A result sent already stands: the panic came while the picture was kept on disk.
            if made.is_err() && sent.get() {
                let fault = Fault::other("keeping a picture on disk panicked; it is shown, not kept");
                ErrorLog::new(errors).record("picture", &fault);
            } else if made.is_err() {
                cached::panicked(&job, cache.as_ref());
                let _ = tx.send((((job.source, job.request), Err(Refused::Unreadable)), true));
            }
        });
    }

    /// The next job that ended, and every other that ended by then (one frame for them all).
    pub(crate) async fn next(&mut self) -> Vec<Done> {
        // Never `None`: this end holds a sender.
        let Some(first) = self.rx.recv().await else { return std::future::pending().await };
        let mut ended = vec![first];
        while let Ok(more) = self.rx.try_recv() {
            ended.push(more);
        }
        ended.into_iter().map(|(done, panicked)| self.panicked(done, panicked)).collect()
    }

    /// `done`, its decoder's panic (`panicked`) written to the error log.
    fn panicked(&self, done: Done, panicked: bool) -> Done {
        if panicked {
            let fault = Fault::other("the picture decoder panicked; the picture is not shown");
            ErrorLog::new(self.errors.clone()).record("picture", &fault);
        }
        done
    }
}

/// `work` on a blocking task; one that reads the disk (`disk`) first waits for one of the
/// `loaders`, held until it ends.
fn blocking(loaders: &Arc<Semaphore>, disk: bool, work: impl FnOnce() + Send + 'static) {
    if !disk {
        tokio::task::spawn_blocking(work);
        return;
    }
    let loaders = loaders.clone();
    tokio::spawn(async move {
        // Never closed: a permit always comes.
        let Ok(_loader) = loaders.acquire_owned().await else { return };
        let _ = tokio::task::spawn_blocking(work).await;
    });
}

/// Write what the terminal is to be sent (pictures, and their removal) before the next frame.
pub(crate) fn write(app: &mut App) -> io::Result<()> {
    if app.images.on {
        // From the first time pictures are on, the way out removes them.
        crate::term::pictures_on(app.images.highest);
    }
    let graphics = app.images.take_graphics();
    if graphics.is_empty() {
        return Ok(());
    }
    // Sent after the ids it names were noted: the way out removes them too.
    crate::term::pictures_on(app.images.highest);
    let mut out = io::stdout();
    out.write_all(graphics.as_bytes())?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn at_most_disk_loaders_reads_run_at_once() {
        let loaders = Arc::new(Semaphore::new(cached::DISK_LOADERS));
        let (running, most, open) =
            (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)));
        let (tx, mut rx) = mpsc::unbounded_channel();
        let reads = cached::DISK_LOADERS * 4;
        for _ in 0..reads {
            let (running, most, open, tx) = (running.clone(), most.clone(), open.clone(), tx.clone());
            blocking(&loaders, true, move || {
                most.fetch_max(running.fetch_add(1, SeqCst) + 1, SeqCst);
                // Held until the test opens the way (or 5 s at most, so nothing hangs).
                let until = Instant::now() + Duration::from_secs(5);
                while !open.load(SeqCst) && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(1));
                }
                running.fetch_sub(1, SeqCst);
                let _ = tx.send(true);
            });
        }
        let until = Instant::now() + Duration::from_secs(5);
        while running.load(SeqCst) < cached::DISK_LOADERS && Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        // A picture fetched from the network is decoded while every loader is busy.
        let decode = tx.clone();
        blocking(&loaders, false, move || {
            let _ = decode.send(false);
        });
        let first = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;
        assert_eq!(first, Ok(Some(false)), "the decode does not wait for the disk");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(running.load(SeqCst), cached::DISK_LOADERS, "the others wait their turn");
        open.store(true, SeqCst);
        for _ in 0..reads {
            assert_eq!(tokio::time::timeout(Duration::from_secs(5), rx.recv()).await, Ok(Some(true)));
        }
        assert_eq!(most.load(SeqCst), cached::DISK_LOADERS, "{reads} reads, at most so many at once");
    }

    #[tokio::test]
    async fn every_picture_ended_by_a_wake_is_taken_at_once() {
        let (tx, rx) = mpsc::unbounded_channel();
        let (errors, cache, diag, panic, panic_keep) = (None, None, Diag::default(), false, false);
        let loaders = Arc::new(Semaphore::new(cached::DISK_LOADERS));
        let mut pictures = Pictures { tx, rx, errors, cache, diag, panic, panic_keep, loaders };
        for n in 0..5u64 {
            let done = ((Source::Emoji(format!("e{n}")), Generation(n)), Err(Refused::NotCached));
            pictures.tx.send((done, false)).unwrap();
        }
        let ended = tokio::time::timeout(Duration::from_secs(5), pictures.next()).await.expect("ended");
        assert_eq!(ended.len(), 5, "one wake, one frame, for all five");
        assert_eq!(ended[4].0.1, Generation(4), "in the order they ended");
    }
}
