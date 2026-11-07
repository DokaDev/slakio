//! `:jump <date>`: the focused conversation (a thread's own conversation) read from that day
//! ([`crate::when`]: `yesterday`, `2026-11-01`, `-3d`, …), its first message then selected and
//! marked; and `:search <words>`: the search popup with those words searched.

use super::super::query::Focus;
use super::{App, Row};
use crate::action::{Action, AppAction};
use slakio_core::i18n::{Label, Msg};
use slakio_core::model::Target;
use std::time::Instant;

/// What follows `:<command>` (empty: nothing yet, or only spaces), when the line is that command
/// with a space after it.
fn arg<'a>(line: &'a str, command: &str) -> Option<&'a str> {
    let rest = line.trim_start().strip_prefix(command)?;
    rest.starts_with(char::is_whitespace).then(|| rest.trim())
}

/// The day of `:jump <date>`.
pub(super) fn jump_arg(line: &str) -> Option<&str> {
    arg(line, "jump")
}

/// The words of `:search <words>`.
pub(super) fn search_arg(line: &str) -> Option<&str> {
    arg(line, "search")
}

impl App {
    /// The palette's row of `:jump <date>`.
    pub(super) fn jump_row(&self) -> Row {
        let day = jump_arg(self.cmdline.text()).unwrap_or_default();
        let shown = if day.is_empty() { self.i18n.label(Label::PaletteJumpArg).to_string() } else { day.to_string() };
        let keys = self.palette_keys(Action::App(AppAction::JumpDate));
        Row { name: format!(":jump {shown}"), label: self.i18n.label(Label::ActionJumpDate).to_string(), keys }
    }

    /// The palette's row of `:search <words>`.
    pub(super) fn search_row(&self) -> Row {
        let words = search_arg(self.cmdline.text()).unwrap_or_default();
        let keys = self.palette_keys(Action::App(AppAction::Search));
        Row { name: format!(":search {words}"), label: self.i18n.label(Label::ActionSearchOpen).to_string(), keys }
    }

    /// `:jump <date>` run: an unknown day is said under the list (the palette stays).
    pub(super) fn run_jump(&mut self, text: &str, now: Instant) {
        let day = jump_arg(text).unwrap_or_default();
        let Some(day) = crate::when::parse(day, crate::time::day(self.model.taken())) else {
            self.cmdline.error = Some(Msg::JumpDateBad { text: day.chars().take(40).collect() });
            return;
        };
        self.cmdline.close();
        let Some(p) = self.work.focused() else { return self.info(Msg::Label(Label::JumpDateNone), now) };
        let target = Target::Conversation {
            workspace: p.target.workspace().clone(),
            conversation: p.target.conversation().clone(),
        };
        let (pane, _) = self.work.jump_date(target, crate::when::start(day));
        if let Some(id) = pane {
            self.work.activate(id);
            self.set_focus(Focus::on(id));
        }
        self.landed(now);
    }

    /// `:search <words>` run: the popup open with them searched.
    pub(super) fn run_search(&mut self, text: &str) {
        let words = search_arg(text).unwrap_or_default().to_string();
        self.cmdline.close();
        self.open_search(Some(&words));
    }
}
