//! The `:` command line: closed, or open with the text typed so far. It owns its text; what a
//! command does is the registry's ([`crate::action::by_command`]).

/// The command line's state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandLine {
    /// `Some(text)` while open.
    text: Option<String>,
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
        self.text = Some(String::new());
    }

    pub fn close(&mut self) {
        self.text = None;
    }

    /// Close it and hand back what was typed.
    pub fn take(&mut self) -> String {
        self.text.take().unwrap_or_default()
    }

    /// Type `s` (control characters, a pasted line break among them, are left out).
    pub fn insert(&mut self, s: &str) {
        if let Some(t) = &mut self.text {
            t.extend(s.chars().filter(|c| !c.is_control()));
        }
    }

    /// Delete the last character; on an empty line, close it (as in vim).
    pub fn backspace(&mut self) {
        match &mut self.text {
            Some(t) if t.is_empty() => self.close(),
            Some(t) => {
                t.pop();
            }
            None => {}
        }
    }
}
