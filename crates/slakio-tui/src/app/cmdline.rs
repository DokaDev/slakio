//! The `:` command line, which is also the command palette: closed, or open with the text typed
//! so far, the entry selected in the list under it ([`crate::app::palette`]) and why the last
//! `Enter` did nothing. It owns its text; what the entries are and what running one does is the
//! palette's.

use slakio_core::i18n::Msg;

/// The command line's state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandLine {
    /// `Some(text)` while open.
    text: Option<String>,
    /// The selected entry of the list.
    pub selected: usize,
    /// The selection was moved by hand: `Enter` runs it even with nothing typed.
    pub picked: bool,
    /// Why the last `Enter` ran nothing (shown under the list until the text changes).
    pub error: Option<Msg>,
}

impl CommandLine {
    pub fn is_open(&self) -> bool {
        self.text.is_some()
    }

    /// The text typed so far (empty when closed).
    pub fn text(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }

    pub fn open(&mut self) {
        *self = Self { text: Some(String::new()), ..Self::default() };
    }

    pub fn close(&mut self) {
        *self = Self::default();
    }

    /// Close it and hand back what was typed.
    pub fn take(&mut self) -> String {
        let text = self.text.take().unwrap_or_default();
        self.close();
        text
    }

    /// Replace the text (a command completed to take its argument); the list starts over.
    pub fn set(&mut self, text: &str) {
        if self.is_open() {
            self.text = Some(text.to_string());
            self.changed();
        }
    }

    /// Type `s` (control characters, a pasted line break among them, are left out).
    pub fn insert(&mut self, s: &str) {
        if let Some(t) = &mut self.text {
            t.extend(s.chars().filter(|c| !c.is_control()));
            self.changed();
        }
    }

    /// Delete the last character; on an empty line, close it (as in vim).
    pub fn backspace(&mut self) {
        match &mut self.text {
            Some(t) if t.is_empty() => self.close(),
            Some(t) => {
                t.pop();
                self.changed();
            }
            None => {}
        }
    }

    /// Move the selection by `by` among `n` entries, round past either end.
    pub fn step(&mut self, by: isize, n: usize) {
        let n = n.max(1) as isize;
        self.selected = (self.selected as isize + by).rem_euclid(n) as usize;
        self.picked = true;
    }

    /// The text changed: the selection goes back to the top, the error goes.
    fn changed(&mut self) {
        self.selected = 0;
        self.picked = false;
        self.error = None;
    }
}
