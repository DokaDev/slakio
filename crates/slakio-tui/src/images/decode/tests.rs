use super::*;
use slakio_world::pictures::png;

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// A PNG whose header says `w` × `h` (its data does not match: it must never be read).
fn header_only(w: u32, h: u32) -> Vec<u8> {
    let mut p = png("x", 1, 1);
    p[16..20].copy_from_slice(&w.to_be_bytes());
    p[20..24].copy_from_slice(&h.to_be_bytes());
    let crc = crc32(&p[12..29]);
    p[29..33].copy_from_slice(&crc.to_be_bytes());
    p
}

fn size_of(d: &Decoded) -> (u32, u32) {
    let img = image::load_from_memory_with_format(&d.png, ImageFormat::Png).unwrap();
    img.dimensions()
}

#[test]
fn a_preview_keeps_its_proportions_within_its_cells() {
    let d = decode(&png("a", 360, 240), Shape::Preview, Some((9, 18))).unwrap();
    // 36 × 10 cells are 324 × 180 px: 360 × 240 fits as 270 × 180, 30 × 10 cells.
    assert!(d.cols <= PREVIEW_COLS && d.rows <= PREVIEW_ROWS, "{d:?}");
    assert_eq!((d.cols, d.rows), (30, 10));
    assert_eq!(size_of(&d), (30 * 9, 10 * 18), "the picture fills its cells");
}

#[test]
fn a_small_picture_is_not_made_larger() {
    let d = decode(&png("a", 20, 10), Shape::Preview, Some((10, 20))).unwrap();
    assert_eq!((d.cols, d.rows), (2, 1));
}

#[test]
fn a_photo_fills_its_slot() {
    let d = decode(&png("a", 72, 72), Shape::Photo { cols: 4, rows: 2 }, Some((9, 18))).unwrap();
    assert_eq!((d.cols, d.rows), (4, 2));
    assert_eq!(size_of(&d), (36, 36));
}

#[test]
fn an_unknown_cell_size_is_guessed_and_a_huge_one_is_sent_smaller() {
    assert!(decode(&png("a", 100, 100), Shape::Preview, None).is_ok());
    assert!(decode(&png("a", 100, 100), Shape::Preview, Some((0, 0))).is_ok());
    let d = decode(&png("a", 100, 100), Shape::Photo { cols: 16, rows: 8 }, Some((200, 400))).unwrap();
    let (w, h) = size_of(&d);
    assert!(w <= 1600 && h <= 1600, "{w}x{h}");
}

#[test]
fn a_picture_too_large_is_refused_from_its_header() {
    assert_eq!(decode(&header_only(SIDE_MAX + 1, 10), Shape::Preview, None), Err(Refused::TooLarge));
    assert_eq!(decode(&header_only(10, SIDE_MAX + 1), Shape::Preview, None), Err(Refused::TooLarge));
    assert_eq!(decode(&header_only(4000, 4000), Shape::Preview, None), Err(Refused::TooLarge), "pixels");
    let mut big = png("a", 2, 2);
    big.resize(BYTES_MAX as usize + 1, 0);
    assert_eq!(decode(&big, Shape::Preview, None), Err(Refused::TooLarge), "bytes");
}

#[test]
fn what_is_not_a_picture_is_refused_never_a_panic() {
    for bytes in [
        Vec::new(),
        b"<html>sign in</html>".to_vec(),
        b"GIF89a".to_vec(),
        png("a", 50, 50)[..60].to_vec(),
        b"RIFF\0\0\0\0WEBPVP8 ".to_vec(),
        header_only(0, 10),
    ] {
        assert_eq!(decode(&bytes, Shape::Preview, None), Err(Refused::Unreadable), "{bytes:?}");
    }
}

proptest::proptest! {
    #[test]
    fn any_bytes_decode_or_are_refused(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..400)) {
        let _ = decode(&bytes, Shape::Preview, None);
        let mut p = png("a", 4, 4);
        let n = p.len();
        for (i, b) in bytes.iter().enumerate() {
            p[8 + i % (n - 8)] ^= b;
        }
        let _ = decode(&p, Shape::Preview, None);
    }
}
