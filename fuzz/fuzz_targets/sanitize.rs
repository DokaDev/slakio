//! The sanitiser on arbitrary input: never panics, leaves nothing a terminal would act on, and
//! sanitising twice changes nothing. Fast, so a run covers many inputs; the work on repeated
//! input is the `sanitize_linear` target's.

#![no_main]

use libfuzzer_sys::fuzz_target;
use slakio_core::sanitize::{BLOCK_MAX_CHARS, LINE_MAX_CHARS, sanitize_block, sanitize_line};

/// A character that must never reach the screen (`\n` is allowed in a block).
fn forbidden(c: char, block: bool) -> bool {
    let control = matches!(c, '\0'..='\x1f' | '\x7f'..='\u{9f}') && !(block && c == '\n');
    let bidi = matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}');
    let invisible = matches!(
        c,
        '\u{AD}' | '\u{115F}' | '\u{1160}' | '\u{3164}' | '\u{FFA0}' | '\u{FFFC}' | '\u{200B}' | '\u{200C}' | '\u{2060}'..='\u{2064}' | '\u{FEFF}' | '\u{E0000}'..='\u{E007F}'
    );
    let private = matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{10FFFF}');
    control || bidi || invisible || private
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let line = sanitize_line(&text);
    let block = sanitize_block(&text);
    assert!(line.as_str().chars().count() <= LINE_MAX_CHARS);
    assert!(block.as_str().chars().count() <= BLOCK_MAX_CHARS);
    assert!(!line.as_str().chars().any(|c| forbidden(c, false)), "{line:?}");
    assert!(!block.as_str().chars().any(|c| forbidden(c, true)), "{block:?}");
    assert_eq!(sanitize_line(line.as_str()), line);
    assert_eq!(sanitize_block(block.as_str()), block);
});
