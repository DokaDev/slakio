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
fn delays_at_or_under_ten_ms_are_a_hundred_and_longer_ones_kept_down_to_twenty() {
    for (ms, shown) in [(0, 100), (10, 100), (20, 20), (50, 50), (90, 90)] {
        let d = decode(&gif(8, 8, 2, ms), Shape::Emoji, CELL).unwrap();
        assert!(d.moving.frames.iter().all(|f| f.delay == Duration::from_millis(shown)), "{ms} ms");
    }
    for (ms, shown) in [(0, 100), (10, 100), (11, 20), (19, 20), (20, 20), (40, 40), (60, 60), (250, 250)] {
        assert_eq!(delay(ms, 1), Duration::from_millis(shown), "{ms} ms");
    }
    assert_eq!(delay(0, 0), DELAY_DEFAULT);
    assert_eq!(delay(30, 2), DELAY_FLOOR, "15 ms, as a fraction");
    assert_eq!(delay(21, 2), DELAY_FLOOR, "10.5 ms is over 10 ms");
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

/// A GIF `side` square of `n` frames drawn as an emoji is (a disc on a plain ground that moves
/// a little each frame), or of noise (`noise`: the worst case for PNG).
fn emoji_gif(side: u32, n: usize, noise: bool) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        enc.set_repeat(Repeat::Infinite).unwrap();
        let mut seed = 7u32;
        for i in 0..n {
            let img = RgbaImage::from_fn(side, side, |x, y| {
                if noise {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    let v = seed.to_be_bytes();
                    return Rgba([v[0], v[1], v[2], 255]);
                }
                let (cx, cy) = (side / 2 + i as u32 % 4, side / 2);
                let inside = (x.abs_diff(cx)).pow(2) + (y.abs_diff(cy)).pow(2) <= (side / 3).pow(2);
                if inside { Rgba([250, 200, 40, 255]) } else { Rgba([0, 0, 0, 0]) }
            });
            enc.encode_frame(GifFrame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(40, 1))).unwrap();
        }
    }
    out
}

#[test]
fn emoji_frame_commands_are_this_large() {
    // A Retina cell (about 20 x 42 pixels); an emoji is two cells wide, one high.
    const RETINA: Option<(u16, u16)> = Some((20, 42));
    let largest = |noise: bool| {
        let d = decode(&emoji_gif(64, 8, noise), Shape::Emoji, RETINA).unwrap();
        assert!(d.moving.moves());
        d.moving.frames.iter().map(|f| crate::images::graphics::transmit(1, 2, 1, &f.png).len()).max().unwrap()
    };
    let (plain, noise) = (largest(false), largest(true));
    let budget = crate::images::animate::budget::ANIM_BYTES_PER_SECOND;
    // Measured 1 487 and 8 100 bytes: eight plain emoji at 25 frames a second fit the budget
    // (no frame skipped), eight of noise do not (frames skipped, at the right speed).
    assert!((1_000..=2_500).contains(&plain), "{plain}");
    assert!(8 * 25 * plain <= budget, "{plain}");
    // Noise does not compress: about the frame's pixels (40 x 42 x 4) in base64.
    assert!((6_000..=12_000).contains(&noise), "{noise}");
    assert!(8 * 25 * noise > budget, "{noise}");
}
