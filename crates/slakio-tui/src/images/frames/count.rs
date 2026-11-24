//! How many frames a GIF or an animated PNG says it has, read from its blocks without decoding
//! a pixel, so one with too many is turned down before any frame is decoded. `None` when the
//! bytes do not read as one (the decoder then says what is wrong).

/// The frames of a GIF: its image descriptors, counted up to its trailer.
pub fn gif(bytes: &[u8]) -> Option<usize> {
    let packed = *bytes.get(10)?;
    let mut at = 13 + table(packed);
    let mut frames = 0;
    loop {
        match *bytes.get(at)? {
            0x3B => return Some(frames),
            0x21 => at = blocks(bytes, at + 2)?,
            0x2C => {
                let packed = *bytes.get(at + 9)?;
                // The descriptor, its own colours, the code size, then the data.
                at = blocks(bytes, at + 10 + table(packed) + 1)?;
                frames += 1;
            }
            _ => return None,
        }
    }
}

/// The bytes of the colour table a GIF's `packed` field says follows.
fn table(packed: u8) -> usize {
    if packed & 0x80 == 0 { 0 } else { 3 << ((packed & 7) + 1) }
}

/// Past the sub-blocks starting at `at` (each its length, then that many bytes; a zero ends them).
fn blocks(bytes: &[u8], mut at: usize) -> Option<usize> {
    loop {
        let len = usize::from(*bytes.get(at)?);
        at += 1;
        if len == 0 {
            return Some(at);
        }
        at += len;
    }
}

/// The frames an animated PNG says it has (its `acTL` chunk, before its pixels).
pub fn apng(bytes: &[u8]) -> Option<usize> {
    let mut at = 8;
    loop {
        let len = usize::try_from(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?)).ok()?;
        match bytes.get(at + 4..at + 8)? {
            b"acTL" => {
                let n = u32::from_be_bytes(bytes.get(at + 8..at + 12)?.try_into().ok()?);
                return usize::try_from(n).ok();
            }
            b"IDAT" | b"IEND" => return None,
            _ => at = at.checked_add(12)?.checked_add(len)?,
        }
    }
}
