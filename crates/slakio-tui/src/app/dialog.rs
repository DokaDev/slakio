//! A question with two answers, over the screen: quit (with a message not sent, or after a
//! click), use Nerd
//! Font icons (asked once), or clear a saved item. The safe answer has the focus, so `Enter` alone never does the risky
//! thing; `y` and `n` answer directly.

use super::{App, Effect};
use crate::action::DialogAction;

/// What is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Question {
    /// A composer holds text that was not sent: quit anyway?
    Quit,
    /// Quit? (Nothing is lost; asked when a click asked to quit.)
    QuitConfirm,
    /// Does the terminal's font show Nerd Font icons? (The answer is saved.)
    Icons,
    /// Clear the saved item under the cursor? (It leaves To-dos and Saved.)
    Clear,
    /// Open a link whose text names another host than the one it goes to?
    Open,
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

impl App {
    /// What is asked, while it is.
    pub fn question(&self) -> Option<Question> {
        self.dialog.map(|d| d.question)
    }

    /// An answer to the question asked (`Tab` moves the focus between the answers).
    pub(super) fn answer(&mut self, a: DialogAction) {
        let Some(mut d) = self.dialog else { return };
        let yes = match a {
            DialogAction::Yes => true,
            DialogAction::No => false,
            DialogAction::Choose => d.yes,
            DialogAction::Toggle => {
                d.yes = !d.yes;
                if d.question == Question::Icons {
                    // The view switcher shows the answer that has the focus.
                    self.settings.icons = d.yes;
                }
                self.dialog = Some(d);
                return;
            }
        };
        self.dialog = None;
        match d.question {
            Question::Quit | Question::QuitConfirm => self.quit = yes,
            Question::Clear => self.answer_clear(yes),
            Question::Open => self.answer_open(yes),
            Question::Icons => {
                self.settings.icons = yes;
                self.effects.push(Effect::Save { key: "icons", value: if yes { "on" } else { "off" }.to_string() });
            }
        }
    }
}
