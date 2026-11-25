use super::*;
use crate::images::decode::{Shape, decode};
use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame as GifFrame, Rgba, RgbaImage};

/// A GIF `w` × `h` of `n` frames (each a colour of its own), each shown `ms` milliseconds.
fn gif(w: u32, h: u32, n: usize, ms: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        enc.set_repeat(Repeat::Infinite).unwrap();
        for i in 0..n {
            let shade = (i * 37 % 256) as u8;
            let img = RgbaImage::from_pixel(w, h, Rgba([shade, 255 - shade, 128, 255]));
            enc.encode_frame(GifFrame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(ms, 1))).unwrap();
        }
    }
    out
}

const CELL: Option<(u16, u16)> = Some((9, 18));
const PHOTO: Shape = Shape::Photo { cols: 4, rows: 2 };

#[test]
fn a_gif_emoji_keeps_its_frames_made_as_its_cells_show_them() {
    let d = decode(&gif(64, 64, 5, 120), Shape::Emoji, CELL).unwrap();
    let m = &d.moving;
    assert!(m.moves());
    assert_eq!((m.frames.len(), m.seen, m.why), (5, 5, ""));
    assert!(m.frames.iter().all(|f| f.delay == Duration::from_millis(120)));
    for f in &m.frames {
        let img = image::load_from_memory_with_format(&f.png, ImageFormat::Png).unwrap();
        assert_eq!((img.width(), img.height()), (18, 18), "made smaller to two cells before it is kept");
    }
}

#[test]
fn a_short_delay_is_shown_a_tenth_of_a_second() {
    for ms in [0, 10, 20, 90] {
        let d = decode(&gif(8, 8, 2, ms), Shape::Emoji, CELL).unwrap();
        assert!(d.moving.frames.iter().all(|f| f.delay == DELAY_MIN), "{ms} ms");
    }
    assert_eq!(delay(0, 0), DELAY_MIN);
    assert_eq!(delay(250, 1), Duration::from_millis(250));
}

#[test]
fn more_than_sixty_four_frames_stays_still_on_its_first() {
    let d = decode(&gif(8, 8, FRAMES_MAX, 100), Shape::Emoji, CELL).unwrap();
    assert_eq!(d.moving.frames.len(), FRAMES_MAX, "64 frames move");
    let d = decode(&gif(8, 8, FRAMES_MAX + 1, 100), Shape::Emoji, CELL).unwrap();
    assert!(!d.moving.moves() && d.moving.frames.is_empty());
    assert_eq!((d.moving.seen, d.moving.why), (FRAMES_MAX + 1, "frames"));
    assert!(!d.png.is_empty(), "its first frame is shown");
}

#[test]
fn frames_past_the_pixel_budget_stay_still() {
    // A photo of 4 × 2 cells at 200 × 400 pixels a cell: 800 × 800 drawn, 2.56 MB a frame.
    let big = Some((200, 400));
    let d = decode(&gif(32, 32, 3, 100), PHOTO, big).unwrap();
    assert_eq!(d.moving.frames.len(), 3, "three fit in 8 MiB");
    let d = decode(&gif(32, 32, 4, 100), PHOTO, big).unwrap();
    assert!(!d.moving.moves());
    assert_eq!((d.moving.seen, d.moving.why), (4, "pixels"));
}

#[test]
fn a_preview_a_still_gif_and_a_png_never_move() {
    let d = decode(&gif(32, 32, 3, 100), Shape::Preview, CELL).unwrap();
    assert_eq!((d.moving.moves(), d.moving.why), (false, "preview"));
    let d = decode(&gif(32, 32, 1, 100), Shape::Emoji, CELL).unwrap();
    assert_eq!((d.moving.moves(), d.moving.why), (false, "single"));
    let d = decode(&slakio_world::pictures::png("x", 32, 32), Shape::Emoji, CELL).unwrap();
    assert_eq!((d.moving.moves(), d.moving.why), (false, "single"));
}

/// `gif`'s frames as one animated PNG (looping), each shown `ms` milliseconds.
fn apng(gif: &[u8], ms: u16) -> Vec<u8> {
    let frames: Vec<RgbaImage> =
        GifDecoder::new(std::io::Cursor::new(gif)).unwrap().into_frames().map(|f| f.unwrap().into_buffer()).collect();
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, frames[0].width(), frames[0].height());
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_animated(u32::try_from(frames.len()).unwrap(), 0).unwrap();
    let mut w = enc.write_header().unwrap();
    for frame in &frames {
        w.set_frame_delay(ms, 1000).unwrap();
        w.write_image_data(frame.as_raw()).unwrap();
    }
    w.finish().unwrap();
    out
}

#[test]
fn an_animated_png_moves_as_a_gif_does() {
    let d = decode(&apng(&gif(40, 40, 4, 150), 150), PHOTO, CELL).unwrap();
    assert_eq!(d.moving.frames.len(), 4);
    assert!(d.moving.frames.iter().all(|f| f.delay == Duration::from_millis(150)));
}

#[test]
fn frames_past_the_source_pixel_budget_are_not_decoded() {
    // Nine frames of 2048 × 2048 drawn as a small emoji: little drawn, much decoded.
    let d = decode(&gif(2048, 2048, 9, 100), Shape::Emoji, CELL).unwrap();
    assert!(!d.moving.moves());
    assert_eq!(d.moving.why, "pixels");
    let each = 2048 * 2048;
    assert!(d.moving.seen as u64 * each <= SOURCE_PIXELS_MAX, "no frame past the budget is read: {}", d.moving.seen);
    assert!(!d.png.is_empty(), "its first frame is shown");
}

#[test]
fn a_frame_past_the_most_is_never_decoded() {
    // Sixty-five frames, the last one broken where its pixels start: counted, not decoded.
    let mut bytes = gif(8, 8, FRAMES_MAX + 1, 100);
    let at = bytes.windows(3).rposition(|w| w == [0x21, 0xF9, 0x04]).expect("the last frame's control block");
    let descriptor = at + 8;
    assert_eq!(bytes[descriptor], 0x2C, "an image descriptor follows");
    let packed = bytes[descriptor + 9];
    let palette = if packed & 0x80 != 0 { 3 << ((packed & 7) + 1) } else { 0 };
    bytes[descriptor + 10 + palette] = 0x0F;
    let d = decode(&bytes, Shape::Emoji, CELL).unwrap();
    assert_eq!((d.moving.seen, d.moving.why), (FRAMES_MAX + 1, "frames"), "not \"unreadable\"");
}

#[test]
fn frames_are_counted_from_the_blocks() {
    assert_eq!(count::gif(&gif(8, 8, 5, 100)), Some(5));
    assert_eq!(count::gif(&gif(8, 8, 5, 100)[..40]), None, "cut short: not counted");
    assert_eq!(count::apng(&apng(&gif(16, 16, 3, 100), 100)), Some(3));
    let d = decode(&gif(16, 16, 3, 100), Shape::Emoji, CELL).unwrap();
    assert_eq!(count::apng(&d.png), None, "one picture");
}
