//! What typing does where no binding claims the key (text into the composer, the command line,
//! the help's filter), and the composer's and the command line's own actions.

use super::composer::Composer;
use super::{App, Label, Msg};
use crate::action::{CommandLineAction, ComposerAction};
use crate::keymap::{Ctx, KeyChord};
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use std::time::Instant;

impl App {
    /// A key no binding claims, for what has the focus.
    pub(super) fn type_key(&mut self, ctx: Ctx, key: KeyChord) {
        let typed = match key.code {
            KeyCode::Char(c) if !key.mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => Some(c),
            _ => None,
        };
        match ctx {
            Ctx::ComposerInsert => {
                let Some(c) = self.work.draft_mut() else { return };
                match (typed, key.code) {
                    (Some(ch), _) => c.insert(ch.encode_utf8(&mut [0; 4])),
                    (_, KeyCode::Backspace) => {
                        c.backspace();
                    }
                    (_, KeyCode::Delete) => {
                        c.delete();
                    }
                    (_, KeyCode::Left) => c.left(),
                    (_, KeyCode::Right) => c.right(),
                    (_, KeyCode::Up) => c.vertical(-1),
                    (_, KeyCode::Down) => c.vertical(1),
                    (_, KeyCode::Home) => c.home(),
                    (_, KeyCode::End) => c.end(),
                    _ => {}
                }
            }
            Ctx::CommandLine => match (typed, key.code) {
                (Some(c), _) => self.cmdline.insert(c.encode_utf8(&mut [0; 4])),
                (_, KeyCode::Backspace) => self.cmdline.backspace(),
                _ => {}
            },
            Ctx::HelpFilter => {
                let Some(h) = self.help.as_mut() else { return };
                match (typed, key.code) {
                    (Some(c), _) => h.filter.push(c),
                    (_, KeyCode::Backspace) if h.filter.is_empty() => h.typing = false,
                    (_, KeyCode::Backspace) => {
                        h.filter.pop();
                    }
                    _ => {}
                }
                h.cursor = 0;
            }
            _ => {}
        }
    }

    pub(super) fn composer(&mut self, a: ComposerAction, now: Instant) {
        match a {
            // Never shown as sent where nothing can be sent.
            ComposerAction::Send if self.read_only() => self.info(Msg::Label(Label::ComposerReadOnly), now),
            ComposerAction::Send => {
                if self.work.send(&self.model) && self.backend.is_some_and(|c| c.demo) {
                    self.info(Msg::Label(Label::StatusDemoEcho), now);
                }
            }
            ComposerAction::Newline => self.work.with_draft(Composer::newline),
            ComposerAction::Leave => self.work.set_insert(false),
            ComposerAction::DeleteWord => self.work.with_draft(|c| {
                c.delete_word_back();
            }),
            ComposerAction::DeleteLine => self.work.with_draft(|c| {
                c.delete_line_back();
            }),
            ComposerAction::DeleteToEnd => self.work.with_draft(|c| {
                c.delete_to_end();
            }),
        }
    }

    pub(super) fn command_line(&mut self, a: CommandLineAction, now: Instant) {
        match a {
            CommandLineAction::Open if !self.checking_sign_in() => {
                self.help = None;
                self.cmdline.open();
            }
            CommandLineAction::Cancel | CommandLineAction::Open => self.cmdline.close(),
            CommandLineAction::Run => self.palette_run(now),
            CommandLineAction::Next => self.palette_step(1),
            CommandLineAction::Prev => self.palette_step(-1),
        }
    }
}
