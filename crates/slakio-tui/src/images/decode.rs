//! A picture's bytes made into what the terminal draws: read, bounded, made smaller, and written
//! again as a PNG that fills a whole number of cells. It runs off the loop's thread (the binary
//! hands it to a blocking task); nothing here waits on anything but the CPU.
//!
//! * At most [`BYTES_MAX`] bytes are read, of a PNG, JPEG or GIF (told from its first bytes,
//!   never from a name); a GIF shows its first frame.
//! * Its size is read from its header first: wider or taller than [`SIDE_MAX`], or more than
//!   [`PIXELS_MAX`] pixels, and it is refused without being decoded; the decoder is held to the
//!   same bounds and to the memory they take.
//! * A preview fits in [`PREVIEW_COLS`] × [`PREVIEW_ROWS`] cells, never made larger than it is
//!   (but at least one cell); a photo fills the comfortable layout's picture slot, square, in its
//!   middle. The cells' size in pixels comes from the terminal (when it says it); the picture
//!   sent is at most [`SENT_SIDE_MAX`] pixels a side.

use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader, Limits, RgbaImage, imageops};
use slakio_core::images::{BYTES_MAX, PIXELS_MAX, SIDE_MAX};
use std::io::Cursor;

/// The most cells a preview takes, across and down.
pub const PREVIEW_COLS: u16 = 36;
pub const PREVIEW_ROWS: u16 = 10;

/// The largest side of a picture sent to the terminal, in pixels.
const SENT_SIDE_MAX: u32 = 1600;

/// A cell's size in pixels when the terminal does not say (a common font's).
pub const CELL_GUESS: (u16, u16) = (9, 18);

/// What a picture is shown as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A file's picture under its card, in its own size up to [`PREVIEW_COLS`] × [`PREVIEW_ROWS`].
    Preview,
    /// A profile photo, `cols` × `rows` cells.
    Photo { cols: u16, rows: u16 },
}

/// A picture ready for the terminal: a PNG that fills `cols` × `rows` cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub png: Vec<u8>,
    pub cols: u16,
    pub rows: u16,
}

/// Why a picture is not shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Larger than slakio reads (bytes, sides or pixels).
    TooLarge,
    /// Not a PNG, JPEG or GIF, or not one that can be read.
    Unreadable,
}

/// The format of `bytes`, when it is one slakio reads.
fn format(bytes: &[u8]) -> Result<ImageFormat, Refused> {
    match image::guess_format(bytes) {
        Ok(f @ (ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif)) => Ok(f),
        _ => Err(Refused::Unreadable),
    }
}

/// `bytes` decoded within the bounds (the header read first).
fn read(bytes: &[u8]) -> Result<DynamicImage, Refused> {
    if bytes.len() as u64 > BYTES_MAX {
        return Err(Refused::TooLarge);
    }
    let format = format(bytes)?;
    let (w, h) =
        ImageReader::with_format(Cursor::new(bytes), format).into_dimensions().map_err(|_| Refused::Unreadable)?;
    if w == 0 || h == 0 {
        return Err(Refused::Unreadable);
    }
    if w > SIDE_MAX || h > SIDE_MAX || u64::from(w) * u64::from(h) > PIXELS_MAX {
        return Err(Refused::TooLarge);
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(SIDE_MAX);
    limits.max_image_height = Some(SIDE_MAX);
    // Four bytes a pixel, and as much again for the decoder's own work.
    limits.max_alloc = Some(PIXELS_MAX * 8);
    reader.limits(limits);
    reader.decode().map_err(|e| match e {
        image::ImageError::Limits(_) => Refused::TooLarge,
        _ => Refused::Unreadable,
    })
}

/// A cell's size: `cell` when the terminal said one that makes sense, else [`CELL_GUESS`].
fn cell_px(cell: Option<(u16, u16)>) -> (u32, u32) {
    let (w, h) = cell.filter(|(w, h)| (2..=200).contains(w) && (2..=400).contains(h)).unwrap_or(CELL_GUESS);
    (u32::from(w), u32::from(h))
}

/// `bytes` as a picture of `shape`, for cells of `cell` pixels (as the terminal says, if it does).
pub fn decode(bytes: &[u8], shape: Shape, cell: Option<(u16, u16)>) -> Result<Decoded, Refused> {
    let img = read(bytes)?;
    let (cw, ch) = cell_px(cell);
    let (w, h) = img.dimensions();
    let (cols, rows, fit) = match shape {
        Shape::Preview => {
            let scale = f64::min(
                f64::from(u32::from(PREVIEW_COLS) * cw) / f64::from(w),
                f64::from(u32::from(PREVIEW_ROWS) * ch) / f64::from(h),
            )
            .min(1.0);
            let (fw, fh) = (((f64::from(w) * scale) as u32).max(1), ((f64::from(h) * scale) as u32).max(1));
            let cols = fw.div_ceil(cw).clamp(1, u32::from(PREVIEW_COLS));
            let rows = fh.div_ceil(ch).clamp(1, u32::from(PREVIEW_ROWS));
            (cols, rows, (fw, fh))
        }
        Shape::Photo { cols, rows } => {
            let side = (u32::from(cols) * cw).min(u32::from(rows) * ch);
            (u32::from(cols), u32::from(rows), (side, side))
        }
    };
    // The canvas the cells cover, no larger than is sent; the picture keeps its proportions.
    let (canvas_w, canvas_h) = (cols * cw, rows * ch);
    let down = f64::min(1.0, f64::from(SENT_SIDE_MAX) / f64::from(canvas_w.max(canvas_h)));
    let px = |v: u32| ((f64::from(v) * down) as u32).max(1);
    let (canvas_w, canvas_h, fit_w, fit_h) = (px(canvas_w), px(canvas_h), px(fit.0), px(fit.1));
    let picture = match shape {
        Shape::Preview => imageops::resize(&img.to_rgba8(), fit_w, fit_h, imageops::FilterType::Triangle),
        Shape::Photo { .. } => {
            let side = w.min(h);
            let square = img.crop_imm((w - side) / 2, (h - side) / 2, side, side).to_rgba8();
            imageops::resize(&square, fit_w, fit_h, imageops::FilterType::Triangle)
        }
    };
    let mut canvas = RgbaImage::new(canvas_w, canvas_h);
    let (x, y) = match shape {
        Shape::Preview => (0, 0),
        Shape::Photo { .. } => ((canvas_w.saturating_sub(fit_w)) / 2, (canvas_h.saturating_sub(fit_h)) / 2),
    };
    imageops::overlay(&mut canvas, &picture, i64::from(x), i64::from(y));
    let mut png = Vec::new();
    canvas.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).map_err(|_| Refused::Unreadable)?;
    let cells = |v: u32| u16::try_from(v).unwrap_or(1);
    Ok(Decoded { png, cols: cells(cols), rows: cells(rows) })
}

#[cfg(test)]
mod tests;
