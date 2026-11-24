//! A picture's bytes made into what the terminal draws: read, bounded, made smaller, and written
//! again as a PNG that fills a whole number of cells. It runs off the loop's thread (the binary
//! hands it to a blocking task); nothing here waits on anything but the CPU.
//!
//! * At most [`BYTES_MAX`] bytes are read, of a PNG, JPEG or GIF (told from its first bytes,
//!   never from a name); a GIF shows its first frame, and a GIF emoji or photo keeps its frames
//!   too, within their own bounds ([`frames`]): a file's preview never moves.
//! * Its size is read from its header first: wider or taller than [`SIDE_MAX`], or more than
//!   [`PIXELS_MAX`] pixels, and it is refused without being decoded; the decoder is held to the
//!   same bounds and to the memory they take.
//! * A preview fits in [`PREVIEW_COLS`] × [`PREVIEW_ROWS`] cells, never made larger than it is
//!   (but at least one cell); a photo fills the comfortable layout's picture slot, square, in its
//!   middle; an emoji fits whole in two cells of a row, in their middle. The cells' size in pixels comes from the terminal (when it says it); the picture
//!   sent is at most [`SENT_SIDE_MAX`] pixels a side.

use super::frames::{self, Moving};
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    /// A file's picture under its card, in its own size up to [`PREVIEW_COLS`] × [`PREVIEW_ROWS`].
    Preview,
    /// A profile photo, `cols` × `rows` cells.
    Photo { cols: u16, rows: u16 },
    /// A workspace's own emoji: two cells of a row, the whole picture in their middle, its
    /// proportions kept (a GIF's frames, [`frames`]).
    Emoji,
}

/// A picture ready for the terminal: a PNG that fills `cols` × `rows` cells; and what it was
/// read from (its width and height in pixels, its format), for the diagnostics; its frames, when
/// it moves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub png: Vec<u8>,
    pub cols: u16,
    pub rows: u16,
    pub from: (u32, u32, &'static str),
    pub moving: Moving,
}

/// Why a picture is not shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Larger than slakio reads (bytes, sides or pixels).
    TooLarge,
    /// Not a PNG, JPEG or GIF, or not one that can be read.
    Unreadable,
    /// Looked for in the cache on disk, and nothing there could be shown: asked for instead.
    NotCached,
}

/// The format of `bytes`, when it is one slakio reads.
fn format(bytes: &[u8]) -> Result<ImageFormat, Refused> {
    match image::guess_format(bytes) {
        Ok(f @ (ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Gif)) => Ok(f),
        _ => Err(Refused::Unreadable),
    }
}

/// The decoder's bounds: the sides and the memory a picture within them takes.
pub(super) fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(SIDE_MAX);
    limits.max_image_height = Some(SIDE_MAX);
    // Four bytes a pixel, and as much again for the decoder's own work.
    limits.max_alloc = Some(PIXELS_MAX * 8);
    limits
}

/// `bytes` decoded within the bounds (the header read first), and its format's name.
fn read(bytes: &[u8]) -> Result<(DynamicImage, &'static str), Refused> {
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
    reader.limits(limits());
    let name = match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpeg",
        _ => "gif",
    };
    let img = reader.decode().map_err(|e| match e {
        image::ImageError::Limits(_) => Refused::TooLarge,
        _ => Refused::Unreadable,
    })?;
    Ok((img, name))
}

/// A cell's size: `cell` when the terminal said one that makes sense, else [`CELL_GUESS`].
fn cell_px(cell: Option<(u16, u16)>) -> (u32, u32) {
    let (w, h) = cell.filter(|(w, h)| (2..=200).contains(w) && (2..=400).contains(h)).unwrap_or(CELL_GUESS);
    (u32::from(w), u32::from(h))
}

/// Where a picture `w` × `h` pixels goes in the cells of `shape`: the cells, the canvas they
/// cover (no larger than is sent), the size the picture is made and where it sits on the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub cols: u32,
    pub rows: u32,
    pub canvas: (u32, u32),
    fit: (u32, u32),
    at: (u32, u32),
    /// A photo: the middle square of the picture is what is made.
    square: bool,
}

/// The [`Layout`] of a picture `w` × `h` pixels as `shape`, for cells of `cell` pixels.
fn layout(w: u32, h: u32, shape: Shape, cell: Option<(u16, u16)>) -> Layout {
    let (cw, ch) = cell_px(cell);
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
        Shape::Emoji => {
            let scale = f64::min(f64::from(2 * cw) / f64::from(w), f64::from(ch) / f64::from(h));
            (2, 1, (((f64::from(w) * scale) as u32).max(1), ((f64::from(h) * scale) as u32).max(1)))
        }
    };
    // The canvas the cells cover, no larger than is sent; the picture keeps its proportions.
    let (canvas_w, canvas_h) = (cols * cw, rows * ch);
    let down = f64::min(1.0, f64::from(SENT_SIDE_MAX) / f64::from(canvas_w.max(canvas_h)));
    let px = |v: u32| ((f64::from(v) * down) as u32).max(1);
    let (canvas_w, canvas_h, fit_w, fit_h) = (px(canvas_w), px(canvas_h), px(fit.0), px(fit.1));
    let at = match shape {
        Shape::Preview => (0, 0),
        Shape::Photo { .. } | Shape::Emoji => {
            ((canvas_w.saturating_sub(fit_w)) / 2, (canvas_h.saturating_sub(fit_h)) / 2)
        }
    };
    let square = matches!(shape, Shape::Photo { .. });
    Layout { cols, rows, canvas: (canvas_w, canvas_h), fit: (fit_w, fit_h), at, square }
}

/// `img` (the whole picture) made as `l` says: the canvas the cells show.
pub(super) fn render(img: &RgbaImage, l: &Layout) -> RgbaImage {
    let (w, h) = img.dimensions();
    let picture = if l.square {
        let side = w.min(h);
        let square = imageops::crop_imm(img, (w - side) / 2, (h - side) / 2, side, side).to_image();
        imageops::resize(&square, l.fit.0, l.fit.1, imageops::FilterType::Triangle)
    } else {
        imageops::resize(img, l.fit.0, l.fit.1, imageops::FilterType::Triangle)
    };
    let mut canvas = RgbaImage::new(l.canvas.0, l.canvas.1);
    imageops::overlay(&mut canvas, &picture, i64::from(l.at.0), i64::from(l.at.1));
    canvas
}

/// `canvas` written as a PNG.
pub(super) fn png_of(canvas: &RgbaImage) -> Result<Vec<u8>, Refused> {
    let mut png = Vec::new();
    canvas.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).map_err(|_| Refused::Unreadable)?;
    Ok(png)
}

/// `bytes` as a picture of `shape`, for cells of `cell` pixels (as the terminal says, if it does);
/// a GIF emoji or photo (or one kept on disk as an animated PNG) with its frames ([`frames`]).
pub fn decode(bytes: &[u8], shape: Shape, cell: Option<(u16, u16)>) -> Result<Decoded, Refused> {
    let (img, format) = read(bytes)?;
    let (w, h) = img.dimensions();
    let l = layout(w, h, shape, cell);
    let png = png_of(&render(&img.to_rgba8(), &l))?;
    let moving = match shape {
        Shape::Preview => Moving::still(1, "preview"),
        Shape::Photo { .. } | Shape::Emoji => frames::read(bytes, &l),
    };
    let cells = |v: u32| u16::try_from(v).unwrap_or(1);
    Ok(Decoded { png, cols: cells(l.cols), rows: cells(l.rows), from: (w, h, format), moving })
}

#[cfg(test)]
mod tests;
