//! The terminal-escape sanitiser, and the two types that make it mandatory.
//!
//! Text from a remote party — message bodies, workspace, channel, section and people names,
//! reaction names, later topics, statuses, file names and link texts — is held as [`Remote`].
//! A `Remote` cannot be drawn: it has no `Display`, no `Deref` and no `AsRef<str>`. The only
//! ways to text are [`Remote::line`] and [`Remote::block`], which run the sanitiser and give
//! [`Safe`] text, and [`Remote::unsanitized`], named so that a search finds every use (it is
//! for comparisons and lookups, never for the screen; a test keeps it out of the UI's drawing
//! code).
//!
//! The sanitiser leaves only text a terminal prints, so nothing remote can move the cursor,
//! change colours, set the window title or the clipboard, switch screens, or reorder what the
//! user reads:
//!
//! * escape sequences are removed whole: CSI, OSC, DCS, SOS, PM, APC and the short `ESC x`
//!   forms, in their 7-bit (`ESC [`) and 8-bit (`U+009B`) spellings; a lone `ESC` goes too;
//! * other control characters (C0, DEL, C1) become `U+FFFD`, so the reader sees that
//!   something was there; line breaks (`\n`, `\r\n`, a lone `\r`, NEL, U+2028, U+2029) become
//!   `\n` in a block and a space in a line; a tab becomes spaces;
//! * bidi embeddings, overrides, isolates and marks, zero-width spaces, joiners and word
//!   joiners, BOMs, invisible operators, tag characters and variation selector supplements are
//!   removed (a zero-width joiner stays between two emoji, where it builds one picture, and one
//!   variation selector stays after a character);
//! * private use code points (the icon fonts the UI itself draws with) and noncharacters become
//!   `U+FFFD`, so remote text cannot pass for the UI's own icons;
//! * soft hyphens, Hangul fillers and the object replacement character, which draw as nothing
//!   or pass for blank names, are removed;
//! * at most [`MAX_MARKS`] combining marks follow a character (Latin, Hebrew and Arabic marks
//!   alike);
//! * a line keeps at most [`LINE_MAX_CHARS`] characters and a block [`BLOCK_MAX_CHARS`]; what is
//!   cut ends with `…`. Only the first [`LINE_MAX_INPUT_BYTES`] or [`BLOCK_MAX_INPUT_BYTES`] of
//!   the input are read at all, and the work is linear in them: a hostile text cannot make the
//!   sanitiser slow.
//!
//! Sanitising twice gives what sanitising once gave.
//!
//! ```
//! use slakio_core::sanitize::Remote;
//! let name = Remote::new("evil\x1b]0;title\x07 name");
//! assert_eq!(name.line().as_str(), "evil name");
//! ```
//!
//! What the compiler rejects:
//!
//! ```compile_fail,E0277
//! // Remote text cannot be formatted for the screen.
//! let r = slakio_core::sanitize::Remote::new("x");
//! let _ = format!("{r}");
//! ```
//!
//! ```compile_fail,E0308
//! // Nor used as a string.
//! let r = slakio_core::sanitize::Remote::new("x");
//! let _: &str = &r;
//! ```

use std::fmt;

/// The most characters a line (a name) keeps.
pub const LINE_MAX_CHARS: usize = 256;
/// The most characters a block (a message body) keeps: Slack's own limit for a message.
pub const BLOCK_MAX_CHARS: usize = 40_000;
/// The most bytes of a line's input that are read; the rest is cut (sixteen bytes for each
/// character kept: room for escape sequences and invisible characters around real text).
pub const LINE_MAX_INPUT_BYTES: usize = LINE_MAX_CHARS * 16;
/// The most bytes of a block's input that are read; the rest is cut.
pub const BLOCK_MAX_INPUT_BYTES: usize = BLOCK_MAX_CHARS * 16;
/// The most combining marks kept after one character.
pub const MAX_MARKS: usize = 2;
/// What a tab becomes in a block.
const TAB: &str = "    ";
/// What ends text that was cut.
const CUT: char = '…';

/// Text from a remote party. It reaches the screen only through [`Remote::line`] or
/// [`Remote::block`].
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Remote(String);

impl Remote {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// Sanitised for one line: names, titles, labels.
    pub fn line(&self) -> Safe {
        sanitize_line(&self.0)
    }

    /// Sanitised with its line breaks: message bodies.
    pub fn block(&self) -> Safe {
        sanitize_block(&self.0)
    }

    /// The text as it came, for comparisons and lookups. Never for the screen.
    pub fn unsanitized(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<&str> for Remote {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for Remote {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl PartialEq<str> for Remote {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Remote {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

/// Escaped, so a test failure or a log line never writes the raw text to a terminal.
impl fmt::Debug for Remote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Remote({:?})", self.0)
    }
}

/// Text that is safe to draw: the sanitiser's output, or text the program wrote itself
/// ([`Safe::trusted`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Safe(String);

impl Safe {
    /// Text this program wrote (never anything remote): the user's own typing after the
    /// composer filtered it, fixed strings. Grep-able like [`Remote::unsanitized`].
    pub fn trusted(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for Safe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `text` sanitised for one line (no line breaks), at most [`LINE_MAX_CHARS`] characters.
pub fn sanitize_line(text: &str) -> Safe {
    Safe(sanitize(text, false, LINE_MAX_CHARS))
}

/// `text` sanitised keeping its line breaks (`\n`), at most [`BLOCK_MAX_CHARS`] characters.
pub fn sanitize_block(text: &str) -> Safe {
    Safe(sanitize(text, true, BLOCK_MAX_CHARS))
}

/// A C0 or C1 control character, or DEL.
fn is_control(c: char) -> bool {
    matches!(c, '\0'..='\x1f' | '\x7f'..='\u{9f}')
}

/// Removed without a trace: bidi controls, zero-width and invisible formatting characters, tag
/// characters, variation selector supplements, and the characters that draw as nothing or pass
/// for a blank name (soft hyphen, Hangul fillers, object replacement). The zero-width joiner and
/// the variation selectors are judged by their neighbours instead.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'                       // soft hyphen
        | '\u{061C}'                     // Arabic letter mark
        | '\u{115F}' | '\u{1160}'        // Hangul choseong and jungseong fillers
        | '\u{180E}'                     // Mongolian vowel separator
        | '\u{200B}'                     // zero-width space
        | '\u{200C}'                     // zero-width non-joiner
        | '\u{200E}' | '\u{200F}'        // left-to-right and right-to-left marks
        | '\u{202A}'..='\u{202E}'        // bidi embeddings and overrides
        | '\u{2060}'..='\u{2064}'        // word joiner, invisible operators
        | '\u{2066}'..='\u{2069}'        // bidi isolates
        | '\u{3164}'                     // Hangul filler
        | '\u{206A}'..='\u{206F}'        // deprecated format characters
        | '\u{FEFF}'                     // byte order mark
        | '\u{FFA0}'                     // halfwidth Hangul filler
        | '\u{FFF9}'..='\u{FFFC}'        // interlinear annotation, object replacement
        | '\u{E0000}'..='\u{E007F}'      // tag characters
        | '\u{E0100}'..='\u{E01EF}' // variation selector supplement
    )
}

/// Private use (icon fonts) and noncharacters.
fn is_unsafe_glyph(c: char) -> bool {
    matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{10FFFF}' | '\u{FDD0}'..='\u{FDEF}')
        || (c as u32) & 0xFFFE == 0xFFFE
}

/// Combining marks (the blocks a "zalgo" flood uses, and Hebrew and Arabic points and
/// cantillation marks, which stack the same way).
fn is_mark(c: char) -> bool {
    matches!(
        c,
        '\u{0300}'..='\u{036F}'
            | '\u{0591}'..='\u{05BD}'
            | '\u{05BF}'
            | '\u{05C1}'..='\u{05C2}'
            | '\u{05C4}'..='\u{05C5}'
            | '\u{05C7}'
            | '\u{0610}'..='\u{061A}'
            | '\u{064B}'..='\u{065F}'
            | '\u{0670}'
            | '\u{06D6}'..='\u{06DC}'
            | '\u{06DF}'..='\u{06E4}'
            | '\u{06E7}'..='\u{06E8}'
            | '\u{06EA}'..='\u{06ED}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
    )
}

fn is_variation_selector(c: char) -> bool {
    matches!(c, '\u{FE00}'..='\u{FE0F}')
}

/// A character a zero-width joiner may join (emoji, their modifiers, the emoji presentation
/// selector).
fn is_emoji(c: char) -> bool {
    matches!(c, '\u{2190}'..='\u{21FF}' | '\u{2300}'..='\u{23FF}' | '\u{2600}'..='\u{27BF}' | '\u{2B00}'..='\u{2BFF}' | '\u{1F000}'..='\u{1FAFF}' | '\u{FE0F}')
}

/// An emoji picture itself (not the presentation selector).
fn is_pictograph(c: char) -> bool {
    is_emoji(c) && c != '\u{FE0F}'
}

/// The character at byte `i` of `s`, if any.
fn char_at(s: &str, i: usize) -> Option<char> {
    s.get(i..).and_then(|r| r.chars().next())
}

/// The input being sanitised and what scanning it has learnt.
struct Scan<'a> {
    s: &'a str,
    /// No string sequence terminator (ST or BEL) at or after this byte: once a scan for one has
    /// run to the end of the text, a later introducer is not scanned again (which would make a
    /// text of unterminated introducers quadratic).
    no_terminator_from: usize,
    /// Characters looked at, for the tests that hold the work linear.
    steps: usize,
}

impl Scan<'_> {
    /// The character at byte `i`, if any.
    fn at(&mut self, i: usize) -> Option<char> {
        self.steps += 1;
        char_at(self.s, i)
    }

    /// The end (byte index) of the escape sequence whose introducer ends at `i`: `csi` for a
    /// control sequence, else a string (OSC, DCS, SOS, PM, APC) ended by ST (`ESC \`, U+009C)
    /// or BEL. `None` when it never ends properly: then only the introducer is dropped.
    fn sequence_end(&mut self, mut i: usize, csi: bool) -> Option<usize> {
        if csi {
            // Parameters and intermediates, then one final byte.
            while let Some(c) = self.at(i) {
                match c {
                    '\x20'..='\x3f' => i += 1,
                    '\x40'..='\x7e' => return Some(i + 1),
                    _ => return None,
                }
            }
            return None;
        }
        if i >= self.no_terminator_from {
            return None;
        }
        let start = i;
        while let Some(c) = self.at(i) {
            match c {
                '\x07' => return Some(i + 1),
                '\u{9c}' => return Some(i + c.len_utf8()),
                '\x1b' if char_at(self.s, i + 1) == Some('\\') => return Some(i + 2),
                _ => i += c.len_utf8(),
            }
        }
        self.no_terminator_from = start;
        None
    }

    /// Where the escape sequence starting with `ESC` at byte `i` ends.
    fn escape_end(&mut self, i: usize) -> usize {
        let lone = i + 1;
        match self.at(lone) {
            Some('[') => self.sequence_end(lone + 1, true).unwrap_or(lone + 1),
            Some(']' | 'P' | 'X' | '^' | '_') => self.sequence_end(lone + 1, false).unwrap_or(lone + 1),
            // `ESC` intermediates final (`ESC ( B`).
            Some('\x20'..='\x2f') => {
                let mut j = lone;
                while let Some('\x20'..='\x2f') = self.at(j) {
                    j += 1;
                }
                match self.at(j) {
                    Some('\x30'..='\x7e') => j + 1,
                    _ => lone,
                }
            }
            // `ESC c` (reset), `ESC 7`, `ESC =`, …
            Some('\x30'..='\x7e') => lone + 1,
            _ => lone,
        }
    }
}

/// `s` cut to at most `max_bytes` (on a character boundary), and whether it was cut.
fn read_at_most(s: &str, max_bytes: usize) -> (&str, bool) {
    if s.len() <= max_bytes {
        return (s, false);
    }
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (&s[..end], true)
}

fn sanitize(s: &str, block: bool, max: usize) -> String {
    sanitize_counted(s, block, max).0
}

/// The characters [`sanitize_block`] (`block`) or [`sanitize_line`] looks at for `text`: at most
/// a few per byte of input, whatever the input. For the fuzz target, which holds it linear.
#[doc(hidden)]
pub fn sanitize_steps(text: &str, block: bool) -> usize {
    sanitize_counted(text, block, if block { BLOCK_MAX_CHARS } else { LINE_MAX_CHARS }).1
}

/// The sanitised text and the characters looked at to make it.
fn sanitize_counted(s: &str, block: bool, max: usize) -> (String, usize) {
    let max_bytes = if block { BLOCK_MAX_INPUT_BYTES } else { LINE_MAX_INPUT_BYTES };
    let (s, truncated) = read_at_most(s, max_bytes);
    let mut scan = Scan { s, no_terminator_from: usize::MAX, steps: 0 };
    let mut out = String::with_capacity(s.len().min(max * 4));
    // Characters in `out`; one more than `max` tells that the text has to be cut.
    let mut kept = 0usize;
    let mut marks = 0usize;
    let mut last: Option<char> = None;
    let mut i = 0;
    while kept <= max {
        let Some(c) = scan.at(i) else { break };
        let next = i + c.len_utf8();
        let start = i;
        i = next;
        let replacement: Option<&str> = match c {
            '\x1b' => {
                i = scan.escape_end(start);
                continue;
            }
            '\u{9b}' => {
                i = scan.sequence_end(next, true).unwrap_or(next);
                continue;
            }
            '\u{90}' | '\u{98}' | '\u{9d}' | '\u{9e}' | '\u{9f}' => {
                i = scan.sequence_end(next, false).unwrap_or(next);
                continue;
            }
            '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                if c == '\r' && char_at(s, next) == Some('\n') {
                    i += 1;
                }
                Some(if block { "\n" } else { " " })
            }
            '\t' => Some(if block { TAB } else { " " }),
            c if is_control(c) || is_unsafe_glyph(c) => Some("\u{FFFD}"),
            c if is_invisible(c) => continue,
            '\u{200D}' if !(last.is_some_and(is_emoji) && char_at(s, next).is_some_and(is_pictograph)) => continue,
            c if is_variation_selector(c)
                && !last.is_some_and(|l| !is_variation_selector(l) && l != '\u{200D}' && l != '\n') =>
            {
                continue;
            }
            c if is_mark(c) => {
                if marks >= MAX_MARKS {
                    continue;
                }
                marks += 1;
                out.push(c);
                kept += 1;
                last = Some(c);
                continue;
            }
            _ => None,
        };
        marks = 0;
        match replacement {
            Some(r) => {
                out.push_str(r);
                kept += r.chars().count();
                last = r.chars().next_back();
            }
            None => {
                out.push(c);
                kept += 1;
                last = Some(c);
            }
        }
    }
    if kept > max || truncated {
        let mut cut: String = out.chars().take(max - 1).collect();
        while cut.ends_with('\u{200D}') {
            cut.pop();
        }
        cut.push(CUT);
        out = cut;
    }
    (out, scan.steps)
}

#[cfg(test)]
mod tests;
