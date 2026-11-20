//! The binary's part of the pictures ([`slakio_tui::images`]): decoding them off the loop's
//! thread (a blocking task each; the app asks for at most a few at once) and writing what the
//! terminal is sent before a frame is drawn.

use ratatui::crossterm::terminal::window_size;
use slakio_core::images::Source;
use slakio_tui::app::App;
use slakio_tui::images::Job;
use slakio_tui::images::decode::{Decoded, Refused, decode};
use std::io::{self, Write};
use tokio::sync::mpsc;

type Done = (Source, Result<Decoded, Refused>);

/// The decodes on their way, and their results.
pub(crate) struct Pictures {
    tx: mpsc::UnboundedSender<Done>,
    rx: mpsc::UnboundedReceiver<Done>,
}

/// A cell's size in pixels, when the terminal says the window's (an `ioctl`, not a question on
/// standard input).
fn cell() -> Option<(u16, u16)> {
    let w = window_size().ok()?;
    (w.columns > 0 && w.rows > 0 && w.width > 0 && w.height > 0).then(|| (w.width / w.columns, w.height / w.rows))
}

impl Pictures {
    pub(crate) fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self { tx, rx }
    }

    /// Decode `job` on a blocking task; its result comes from [`Pictures::next`].
    pub(crate) fn start(&self, job: Job) {
        let (tx, cell) = (self.tx.clone(), cell());
        tokio::task::spawn_blocking(move || {
            // A panic in the decoder is a picture that is not shown, never the app's end.
            let result =
                std::panic::catch_unwind(|| decode(&job.bytes, job.shape, cell)).unwrap_or(Err(Refused::Unreadable));
            let _ = tx.send((job.source, result));
        });
    }

    /// The next decode that ended.
    pub(crate) async fn next(&mut self) -> Done {
        match self.rx.recv().await {
            Some(done) => done,
            // Never: this end holds a sender.
            None => std::future::pending().await,
        }
    }
}

/// Write what the terminal is to be sent (pictures, and their removal) before the next frame.
pub(crate) fn write(app: &mut App) -> io::Result<()> {
    if app.images.on {
        // From the first time pictures are on, the way out removes them.
        crate::term::pictures_on();
    }
    let graphics = app.images.take_graphics();
    if graphics.is_empty() {
        return Ok(());
    }
    let mut out = io::stdout();
    out.write_all(graphics.as_bytes())?;
    out.flush()
}
