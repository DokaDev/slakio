//! A question with two answers, over the screen: quit with a message not sent, or use Nerd
//! Font icons (asked once). The safe answer has the focus, so `Enter` alone never does the risky
//! thing; `y` and `n` answer directly.

/// What is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Question {
    /// A composer holds text that was not sent: quit anyway?
    Quit,
    /// Does the terminal's font show Nerd Font icons? (The answer is saved.)
    Icons,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dialog {
    pub question: Question,
    /// The focus is on "yes".
    pub yes: bool,
}

impl Dialog {
    pub fn new(question: Question) -> Self {
        Self { question, yes: false }
    }
}
