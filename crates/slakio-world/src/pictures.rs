//! The world's pictures: profile photos for most people (none for bots), and the smaller copies
//! its PNG files are previewed by. Every picture is invented here, a PNG drawn from its name
//! ([`png`]), so the demo and the tests' fake Slack serve the same bytes and no real picture is
//! ever used. The addresses are on the real hosts' names ([`AVATAR_HOSTS`], the file host) so
//! they pass the same checks as Slack's; the fake serves them from its loopback address.

use slakio_core::images::AVATAR_HOSTS;
use slakio_core::model::FILE_HOST;

/// The side of a profile photo, in pixels (Slack's `image_72`).
pub const AVATAR_SIDE: u32 = 72;

/// The size of a file's smaller copy, in pixels.
pub const THUMB_SIZE: (u32, u32) = (360, 240);

/// FNV-1a of `s`.
fn hash(s: &str) -> u32 {
    s.bytes().fold(0x811C_9DC5, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
}

/// The profile photo of person `id` of workspace `ws`: two people in three have one; bots none.
pub fn avatar(ws: &slakio_core::model::WorkspaceId, id: &str, bot: bool) -> Option<String> {
    (!bot && !hash(id).is_multiple_of(3)).then(|| format!("https://{}/avatars/{ws}/{id}-72.png", AVATAR_HOSTS[0]))
}

/// The smaller copy of file `id` (named `name`, of workspace `team`), when it is a picture.
pub fn thumb(team: &str, id: &str, name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif"]
        .iter()
        .any(|e| lower.ends_with(e))
        .then(|| format!("https://{FILE_HOST}/files-tmb/{team}-{id}/thumb_360.png"))
}

/// The picture at `path` (an address's path the world made), as PNG bytes: a photo for an
/// `/avatars/` path, a file's copy for a `/files-tmb/` one; `None` for any other.
pub fn served(path: &str) -> Option<Vec<u8>> {
    if path.starts_with("/avatars/") {
        return Some(png(path, AVATAR_SIDE, AVATAR_SIDE));
    }
    path.starts_with("/files-tmb/").then(|| png(path, THUMB_SIZE.0, THUMB_SIZE.1))
}

/// A `w` × `h` picture drawn from `seed` (two colours in bands and a disc), as an uncompressed
/// PNG (stored zlib blocks): the same bytes every time.
pub fn png(seed: &str, w: u32, h: u32) -> Vec<u8> {
    let k = hash(seed);
    let a = [(k >> 16) as u8, (k >> 8) as u8, k as u8];
    let b = [255 - a[1], a[2] / 2 + 64, 255 - a[0]];
    let (cx, cy, r) = (w as i64 / 2, h as i64 / 2, i64::from(w.min(h)) / 3);
    let mut raw = Vec::with_capacity(((w * 3 + 1) * h) as usize);
    for y in 0..i64::from(h) {
        raw.push(0);
        for x in 0..i64::from(w) {
            let disc = (x - cx).pow(2) + (y - cy).pow(2) <= r * r;
            let band = ((x + y) / 12) % 2 == 0;
            raw.extend_from_slice(if disc || band { &a } else { &b });
        }
    }
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// `data` as a zlib stream of stored (uncompressed) blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = data.chunks(65_535).collect();
    for (i, block) in blocks.iter().enumerate() {
        out.push(u8::from(i + 1 == blocks.len()));
        let len = u16::try_from(block.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    let (mut s1, mut s2) = (1u32, 0u32);
    for &byte in data {
        s1 = (s1 + u32::from(byte)) % 65_521;
        s2 = (s2 + s1) % 65_521;
    }
    out.extend_from_slice(&((s2 << 16) | s1).to_be_bytes());
    out
}

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

#[cfg(test)]
mod tests;
