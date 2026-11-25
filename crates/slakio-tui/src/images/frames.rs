//! A picture that moves: the frames of a GIF emoji or photo (or of an animated PNG), each made as the cells show it ([`super::decode::render`]) and written
//! as a PNG, off the loop's thread like the rest of a decode. The loop only sends the frame due
//! ([`super::animate`]); nothing is fetched for it.
//!
//! Bounded: at most [`FRAMES_MAX`] frames, and their pixels as drawn (four bytes each) at most
//! [`FRAME_BYTES_MAX`] in all, their pixels as they come at most [`SOURCE_PIXELS_MAX`] in all
//! (checked before a frame is decoded); a picture past any, or one whose frames stop reading part way,
//! stays still on its first frame (the diagnostics' `decoded` line says why). A frame is shown
//! at least [`DELAY_MIN`] (a GIF that says 0 or 10 ms is shown at ten frames a second, as
//! browsers do).

use super::decode::{Layout, limits, png_of, render};
use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::{AnimationDecoder, Frames, ImageDecoder, ImageFormat};
use std::io::Cursor;
use std::time::Duration;

/// The most frames a picture moves through; one with more stays still.
pub const FRAMES_MAX: usize = 64;
/// The most bytes of a picture's frames as drawn (width × height × 4 each, all frames).
pub const FRAME_BYTES_MAX: u64 = 8 * 1024 * 1024;
/// The most pixels of a picture's frames as they come (width × height each, all frames): what
/// decoding them costs, whatever size they are drawn at.
pub const SOURCE_PIXELS_MAX: u64 = 8 * slakio_core::images::PIXELS_MAX;
/// The shortest time a frame is shown: at most ten frames a second.
pub const DELAY_MIN: Duration = Duration::from_millis(100);

/// One frame as sent: a PNG of the cells, and how long it is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub png: Vec<u8>,
    pub delay: Duration,
}

/// A picture's frames (none: it stays still), how many were read, and why it stays still.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Moving {
    pub frames: Vec<Frame>,
    pub seen: usize,
    pub why: &'static str,
}

impl Moving {
    /// A picture that stays still, after `seen` frames, for `why`.
    pub fn still(seen: usize, why: &'static str) -> Self {
        Self { frames: Vec::new(), seen, why }
    }

    /// It moves.
    pub fn moves(&self) -> bool {
        self.frames.len() > 1
    }

    /// The bytes of its frames as sent (PNG).
    pub fn bytes(&self) -> usize {
        self.frames.iter().map(|f| f.png.len()).sum()
    }
}

/// A frame's delay of `numer` / `denom` milliseconds, at least [`DELAY_MIN`].
pub fn delay(numer: u32, denom: u32) -> Duration {
    let ms = if denom == 0 { 0 } else { u64::from(numer) / u64::from(denom) };
    Duration::from_millis(ms).max(DELAY_MIN)
}

/// The frames of `bytes` (a GIF, or an animated PNG), each made as `l` says, within the bounds.
pub(super) fn read(bytes: &[u8], l: &Layout) -> Moving {
    // Too many frames is known before any is decoded.
    let counted = match image::guess_format(bytes) {
        Ok(ImageFormat::Gif) => count::gif(bytes),
        Ok(ImageFormat::Png) => count::apng(bytes),
        _ => None,
    };
    if let Some(n) = counted.filter(|n| *n > FRAMES_MAX) {
        return Moving::still(n, "frames");
    }
    let frames = match image::guess_format(bytes) {
        Ok(ImageFormat::Gif) => {
            let Ok(mut d) = GifDecoder::new(Cursor::new(bytes)) else { return Moving::still(0, "unreadable") };
            if d.set_limits(limits()).is_err() {
                return Moving::still(0, "unreadable");
            }
            (d.dimensions(), d.into_frames())
        }
        Ok(ImageFormat::Png) => match PngDecoder::with_limits(Cursor::new(bytes), limits()) {
            Ok(d) if d.is_apng().unwrap_or(false) => {
                let size = d.dimensions();
                match d.apng() {
                    Ok(a) => (size, a.into_frames()),
                    Err(_) => return Moving::still(0, "unreadable"),
                }
            }
            _ => return Moving::still(1, "single"),
        },
        _ => return Moving::still(1, "format"),
    };
    let ((w, h), frames) = frames;
    collect(frames, u64::from(w) * u64::from(h), l)
}

/// `frames`, each `source` pixels as they come, made as `l` says, while within the bounds: a
/// frame that would take them past [`SOURCE_PIXELS_MAX`] is not decoded.
fn collect(frames: Frames<'_>, source: u64, l: &Layout) -> Moving {
    let each = u64::from(l.canvas.0) * u64::from(l.canvas.1) * 4;
    let mut raw = Vec::new();
    let mut delays = Vec::new();
    let mut frames = frames.into_iter();
    for seen in 1.. {
        if seen as u64 * source > SOURCE_PIXELS_MAX {
            return Moving::still(seen - 1, "pixels");
        }
        let Some(frame) = frames.next() else { break };
        let Ok(frame) = frame else { return Moving::still(seen, "unreadable") };
        if seen > FRAMES_MAX {
            return Moving::still(seen, "frames");
        }
        if seen as u64 * each > FRAME_BYTES_MAX {
            return Moving::still(seen, "pixels");
        }
        let (numer, denom) = frame.delay().numer_denom_ms();
        delays.push(delay(numer, denom));
        raw.push(render(&frame.into_buffer(), l));
    }
    if raw.len() < 2 {
        return Moving::still(raw.len(), "single");
    }
    let mut out = Vec::with_capacity(raw.len());
    for (canvas, delay) in raw.iter().zip(delays) {
        match png_of(canvas) {
            Ok(png) => out.push(Frame { png, delay }),
            Err(_) => return Moving::still(raw.len(), "unreadable"),
        }
    }
    Moving { seen: raw.len(), frames: out, why: "" }
}

mod count;

#[cfg(test)]
mod tests;
