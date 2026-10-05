//! A pane's composer: multiline text with a cursor, edited by grapheme cluster so Korean
//! syllables and emoji sequences are never split. Text arrives typed, committed by an IME or
//! pasted; whatever arrives goes through the sanitiser first, so the composer never holds an
//! escape sequence or a bidi control, whoever put it on the clipboard.
//!
//! The view wraps lines by cell width ([`Composer::view`]) and says where the cursor is, so the
//! terminal's own cursor (and an IME's preedit) sits exactly where the next character goes.

use crate::text::grapheme_width;
use slakio_core::sanitize::{BLOCK_MAX_CHARS, sanitize_block};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Composer {
    text: String,
    /// Byte index, always at a grapheme boundary.
    cursor: usize,
}

/// The composer laid out for a width: its lines and the cursor's (line, column).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub lines: Vec<String>,
    pub cursor: (usize, usize),
}

impl Composer {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The cursor (a byte index).
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Insert `s` at the cursor, sanitised (a pasted escape sequence or bidi control never gets
    /// in) and cut to what fits in one message.
    pub fn insert(&mut self, s: &str) {
        let clean = sanitize_block(s).into_string();
        let room = BLOCK_MAX_CHARS.saturating_sub(self.text.chars().count());
        let clean: String = clean.chars().take(room).collect();
        self.text.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
    }

    pub fn newline(&mut self) {
        self.insert("\n");
    }

    /// Empty it and hand back what was written.
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    fn prev_boundary(&self) -> Option<usize> {
        self.text[..self.cursor].grapheme_indices(true).next_back().map(|(i, _)| i)
    }

    fn next_boundary(&self) -> Option<usize> {
        self.text[self.cursor..].graphemes(true).next().map(|g| self.cursor + g.len())
    }

    pub fn backspace(&mut self) -> bool {
        match self.prev_boundary() {
            Some(i) => {
                self.text.drain(i..self.cursor);
                self.cursor = i;
                true
            }
            None => false,
        }
    }

    pub fn delete(&mut self) -> bool {
        match self.next_boundary() {
            Some(j) => {
                self.text.drain(self.cursor..j);
                true
            }
            None => false,
        }
    }

    pub fn left(&mut self) {
        if let Some(i) = self.prev_boundary() {
            self.cursor = i;
        }
    }

    pub fn right(&mut self) {
        if let Some(j) = self.next_boundary() {
            self.cursor = j;
        }
    }

    /// The start of the cursor's line.
    fn line_start(&self) -> usize {
        self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1)
    }

    /// The end of the cursor's line.
    fn line_end(&self) -> usize {
        self.text[self.cursor..].find('\n').map_or(self.text.len(), |i| self.cursor + i)
    }

    pub fn home(&mut self) {
        self.cursor = self.line_start();
    }

    pub fn end(&mut self) {
        self.cursor = self.line_end();
    }

    /// Move to the line above (`-1`) or below (`1`), keeping the column in graphemes.
    pub fn vertical(&mut self, step: i8) {
        let start = self.line_start();
        let column = self.text[start..self.cursor].graphemes(true).count();
        let target = if step < 0 {
            if start == 0 {
                return;
            }
            self.text[..start - 1].rfind('\n').map_or(0, |i| i + 1)
        } else {
            let end = self.line_end();
            if end == self.text.len() {
                return;
            }
            end + 1
        };
        let line_end = self.text[target..].find('\n').map_or(self.text.len(), |i| target + i);
        let offset: usize = self.text[target..line_end].graphemes(true).take(column).map(str::len).sum();
        self.cursor = target + offset;
    }

    /// Delete the word before the cursor (`Ctrl+W`): the blanks before it, then a run of
    /// letters, digits and `_`, or of other symbols. Never past the start of the line.
    pub fn delete_word_back(&mut self) -> bool {
        let start = self.line_start();
        let gs: Vec<(usize, &str)> = self.text[start..self.cursor].grapheme_indices(true).collect();
        let word = |g: &str| g.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_');
        let blank = |g: &str| g.chars().all(char::is_whitespace);
        let mut a = gs.len();
        while a > 0 && blank(gs[a - 1].1) {
            a -= 1;
        }
        if a > 0 {
            let w = word(gs[a - 1].1);
            while a > 0 && word(gs[a - 1].1) == w && !blank(gs[a - 1].1) {
                a -= 1;
            }
        }
        if a == gs.len() {
            // At the start of a line: join it to the line above.
            return self.backspace();
        }
        let from = start + gs.get(a).map_or(self.cursor - start, |g| g.0);
        self.text.drain(from..self.cursor);
        self.cursor = from;
        true
    }

    /// Delete from the start of the line to the cursor (`Ctrl+U`).
    pub fn delete_line_back(&mut self) -> bool {
        let start = self.line_start();
        if start == self.cursor {
            return false;
        }
        self.text.drain(start..self.cursor);
        self.cursor = start;
        true
    }

    /// Delete from the cursor to the end of the line (`Ctrl+K`).
    pub fn delete_to_end(&mut self) -> bool {
        let end = self.line_end();
        if end == self.cursor {
            return false;
        }
        self.text.drain(self.cursor..end);
        true
    }

    /// The lines for a width of `w` cells (a line that fills `w` wraps, so the cursor always has
    /// a cell), and where the cursor is.
    pub fn view(&self, w: usize) -> View {
        let w = w.max(2);
        let mut lines = vec![String::new()];
        let mut col = 0;
        let mut cursor = (0, 0);
        let mut at = 0;
        for g in self.text.graphemes(true) {
            if at == self.cursor {
                cursor = (lines.len() - 1, col);
            }
            at += g.len();
            if g == "\n" {
                lines.push(String::new());
                col = 0;
                continue;
            }
            let gw = grapheme_width(g);
            if col + gw > w {
                lines.push(String::new());
                col = 0;
            }
            lines.last_mut().expect("never empty").push_str(g);
            col += gw;
            if col >= w {
                lines.push(String::new());
                col = 0;
            }
        }
        if at == self.cursor {
            cursor = (lines.len() - 1, col);
        }
        // A line that filled the width opened an empty one the cursor does not need.
        if lines.len() > 1 && lines.last().is_some_and(String::is_empty) && cursor.0 < lines.len() - 1 {
            let filled = self.text.ends_with('\n');
            if !filled {
                lines.pop();
            }
        }
        View { lines, cursor }
    }
}

#[cfg(test)]
mod tests;
