//! The application state. `App` is a thin router: each part of the state lives in a sub-state
//! that owns its data and its update ([`cmdline::CommandLine`], [`status::Status`]); `App`
//! turns input into [`Action`]s through the key map and routes each action to its owner. State
//! does no I/O: the binary owns the terminal and the event loop, and reads [`App::quit`] and
//! [`App::deadline`].

pub mod cmdline;
pub mod status;

use crate::action::{self, Action, AppAction, CommandLineAction};
use crate::keymap::{Ctx, KeyChord, Keymap};
use crate::theme::Theme;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use slakio_core::i18n::{I18n, Lang, Msg};
use status::{Level, Status};
use std::time::Instant;

/// The input mode, shown by the badge at the left of the status line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Command,
}

pub struct App {
    pub keymap: Keymap,
    pub i18n: I18n,
    pub theme: Theme,
    pub cmdline: cmdline::CommandLine,
    pub status: Status,
    /// Set once the user asked to quit; the binary's loop ends.
    pub quit: bool,
}

impl App {
    pub fn new(lang: Lang, theme: Theme) -> Self {
        Self {
            keymap: Keymap::default(),
            i18n: I18n::new(lang),
            theme,
            cmdline: cmdline::CommandLine::default(),
            status: Status::default(),
            quit: false,
        }
    }

    pub fn mode(&self) -> Mode {
        if self.cmdline.is_open() { Mode::Command } else { Mode::Normal }
    }

    /// Where the keyboard is.
    pub fn key_context(&self) -> Ctx {
        if self.cmdline.is_open() { Ctx::CommandLine } else { Ctx::Root }
    }

    /// Show a warning in the status line.
    pub fn warn(&mut self, msg: impl Into<Msg>, now: Instant) {
        self.status.show(msg, Level::Warning, now);
    }

    /// Handle a terminal event at `now`. `true` when the screen may have changed.
    pub fn handle_event(&mut self, ev: Event, now: Instant) -> bool {
        match ev {
            // Release and repeat events are reported only with the kitty flags this app never
            // asks for; a press is what counts.
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                let key = KeyChord::from_event(&k);
                let ctx = self.key_context();
                match self.keymap.resolve(ctx, key) {
                    Some(a) => self.dispatch(a, now),
                    None => self.type_key(ctx, key),
                }
                true
            }
            Event::Paste(text) => {
                self.cmdline.insert(&text);
                true
            }
            Event::Resize(..) => true,
            _ => false,
        }
    }

    /// A key no binding claims, for what has the focus.
    fn type_key(&mut self, ctx: Ctx, key: KeyChord) {
        if ctx != Ctx::CommandLine {
            return;
        }
        match key.code {
            KeyCode::Char(c) if !key.mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                self.cmdline.insert(c.encode_utf8(&mut [0; 4]));
            }
            KeyCode::Backspace => self.cmdline.backspace(),
            _ => {}
        }
    }

    /// Route `action` to the sub-state that owns it.
    pub fn dispatch(&mut self, action: Action, now: Instant) {
        match action {
            Action::App(AppAction::Quit) => self.quit = true,
            Action::CommandLine(a) => self.command_line(a, now),
        }
    }

    fn command_line(&mut self, a: CommandLineAction, now: Instant) {
        match a {
            CommandLineAction::Open => self.cmdline.open(),
            CommandLineAction::Cancel => self.cmdline.close(),
            CommandLineAction::Run => {
                let text = self.cmdline.take();
                let name = text.trim();
                if name.is_empty() {
                    return;
                }
                match action::by_command(name) {
                    Some(a) => self.dispatch(a, now),
                    None => self.warn(Msg::CommandUnknown { name: name.to_string() }, now),
                }
            }
        }
    }

    /// When the screen changes next by itself, if ever (the loop sleeps until then).
    pub fn deadline(&self) -> Option<Instant> {
        self.status.deadline()
    }

    /// The deadline passed. `true` when the screen changed.
    pub fn on_tick(&mut self, now: Instant) -> bool {
        self.status.expire(now)
    }
}
