//! Hangul written as conjoining jamo (a Mac writes a file name, so an uploaded emoji's name,
//! in Unicode's decomposed form, NFD) composed into the precomposed syllables (U+AC00 to
//! U+D7A3) the same name has when typed (NFC), so the two are one name.
//!
//! Only modern Hangul composes: a leading consonant (U+1100 to U+1112), a vowel (U+1161 to
//! U+1175) and maybe a trailing consonant (U+11A8 to U+11C2), or a syllable with no trailing
//! consonant followed by one. Everything else stays as it is: other scripts' decomposed forms
//! (a Latin letter and a combining accent), the archaic jamo, the compatibility jamo
//! (U+3131 on), and a jamo that composes with nothing; [`super::shortcode`] still refuses the
//! jamo left over.

use std::borrow::Cow;

const L_FIRST: u32 = 0x1100;
const V_FIRST: u32 = 0x1161;
const T_BEFORE: u32 = 0x11A7;
const S_FIRST: u32 = 0xAC00;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const S_COUNT: u32 = L_COUNT * V_COUNT * T_COUNT;

/// `s` with its modern Hangul jamo composed into syllables; `s` itself (borrowed, byte for
/// byte) when it holds no conjoining jamo at all.
pub fn compose_hangul(s: &str) -> Cow<'_, str> {
    if !s.chars().any(|c| ('\u{1100}'..='\u{11FF}').contains(&c)) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut last: Option<char> = None;
    for c in s.chars() {
        match last.and_then(|l| joined(l, c)) {
            Some(syllable) => last = Some(syllable),
            None => {
                out.extend(last);
                last = Some(c);
            }
        }
    }
    out.extend(last);
    Cow::Owned(out)
}

/// The syllable `last` and `c` compose into, if they do: a leading consonant and a vowel, or a
/// syllable without a trailing consonant and one.
fn joined(last: char, c: char) -> Option<char> {
    let (last, c) = (u32::from(last), u32::from(c));
    let l = last.wrapping_sub(L_FIRST);
    let v = c.wrapping_sub(V_FIRST);
    if l < L_COUNT && v < V_COUNT {
        return char::from_u32(S_FIRST + (l * V_COUNT + v) * T_COUNT);
    }
    let s = last.wrapping_sub(S_FIRST);
    let t = c.wrapping_sub(T_BEFORE);
    if s < S_COUNT && s % T_COUNT == 0 && (1..T_COUNT).contains(&t) {
        return char::from_u32(last + t);
    }
    None
}

#[cfg(test)]
mod tests;
