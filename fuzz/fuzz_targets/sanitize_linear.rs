//! The sanitiser's work on repeated input (the shape that makes a rescanning sanitiser
//! quadratic): the input repeated up to [`REPEATED_MAX`] bytes, the steps counted stay a few per
//! byte read. Capped well below the input limits, so each run stays quick and a run covers many
//! shapes; the `sanitize` target covers the invariants on arbitrary input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use slakio_core::sanitize::{BLOCK_MAX_INPUT_BYTES, LINE_MAX_INPUT_BYTES, sanitize_steps};

/// The most the input is repeated to.
const REPEATED_MAX: usize = 32 * 1024;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if text.is_empty() {
        return;
    }
    let repeated = text.repeat(REPEATED_MAX / text.len() + 1);
    let cut = repeated.floor_char_boundary(REPEATED_MAX.max(text.len()));
    let repeated = &repeated[..cut];
    for (is_block, max_bytes) in [(false, LINE_MAX_INPUT_BYTES), (true, BLOCK_MAX_INPUT_BYTES)] {
        let steps = sanitize_steps(repeated, is_block);
        assert!(steps <= 4 * repeated.len().min(max_bytes) + 16, "{steps} steps for {} bytes", repeated.len());
    }
});
