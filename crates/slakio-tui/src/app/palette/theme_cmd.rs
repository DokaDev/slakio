//! `:theme copy <from> [name]`, `:theme new <name>` and `:theme update [name]`: write theme
//! files ([`crate::theme::authoring`]). After a copy or a new file slakio draws with it and saves
//! `theme = "<name>"`; an update leaves the theme and the config file as they are. What cannot be
//! done is said in the palette, and nothing is written. `:theme <name>` stays as it was.

use super::{Row, theme_arg};
use crate::app::App;
use crate::theme::NAMES;
use crate::theme::authoring::{self, AuthorError, Updated};
use slakio_core::i18n::{Label, Msg};
use slakio_core::sanitize::Remote;
use std::path::Path;
use std::time::Instant;

/// A `:theme` subcommand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeCommand<'a> {
    Copy { from: &'a str, name: Option<&'a str> },
    New { name: &'a str },
    Update { name: Option<&'a str> },
}

/// The subcommand of the argument of `:theme` (`copy dark mine`); `Some(Err(()))`: a subcommand
/// with the wrong number of words; `None`: none (a theme's name, as `:theme new` alone is too).
pub fn parse(arg: &str) -> Option<Result<ThemeCommand<'_>, ()>> {
    let words: Vec<&str> = arg.split_whitespace().collect();
    let first = words.first()?.to_ascii_lowercase();
    Some(match (first.as_str(), &words[..]) {
        ("copy", [_, from]) => Ok(ThemeCommand::Copy { from, name: None }),
        ("copy", [_, from, name]) => Ok(ThemeCommand::Copy { from, name: Some(name) }),
        ("new", [_, name]) => Ok(ThemeCommand::New { name }),
        ("update", [_]) => Ok(ThemeCommand::Update { name: None }),
        ("update", [_, name]) => Ok(ThemeCommand::Update { name: Some(name) }),
        ("copy" | "new" | "update", [_, _, ..]) | ("copy", [_]) => Err(()),
        _ => return None,
    })
}

/// The subcommand of command line `line` (`:theme copy …`, also `:colo copy …`).
pub(super) fn of_line(line: &str) -> Option<Result<ThemeCommand<'_>, ()>> {
    theme_arg(line).and_then(parse)
}

impl App {
    /// The built-in theme `:theme new` extends: the one set when it is built in (`auto` as what
    /// it picks on this terminal, never `no-color` nor a theme only shown), else `terminal`.
    fn new_base(&self) -> String {
        let lower = self.theme_setting.to_ascii_lowercase();
        if lower == "auto" {
            return crate::theme::resolve("auto", self.look.truecolor, self.look.background).name.to_string();
        }
        if NAMES.contains(&lower.as_str()) { lower } else { "terminal".to_string() }
    }

    /// The palette's row of the `:theme` subcommand typed.
    pub(super) fn theme_cmd_row(&self) -> Row {
        let text = self.cmdline.text().trim().to_string();
        let label = match of_line(&text) {
            Some(Ok(ThemeCommand::Copy { from, name })) => {
                let name = name.map_or_else(|| self.copy_name(from), str::to_string);
                Msg::PaletteThemeCopy { from: shown(from), name: shown(&name) }
            }
            Some(Ok(ThemeCommand::New { name })) => Msg::PaletteThemeNew { name: shown(name), base: self.new_base() },
            Some(Ok(ThemeCommand::Update { name })) => {
                Msg::PaletteThemeUpdate { name: shown(name.unwrap_or(&self.theme_setting)) }
            }
            _ => Msg::Label(Label::PaletteThemeUsage),
        };
        Row { name: format!(":{text}"), label: self.i18n.msg(&label).to_string(), keys: String::new() }
    }

    /// `Enter` on a `:theme` subcommand: done (the palette closes), or why not said in it.
    pub(super) fn run_theme_cmd(&mut self, text: &str, now: Instant) {
        let done = match of_line(text) {
            Some(Ok(ThemeCommand::Copy { from, name })) => {
                let name = name.map_or_else(|| self.copy_name(from), str::to_string);
                self.copy_theme(from, &name, now)
            }
            Some(Ok(ThemeCommand::New { name })) => self.new_theme(name, now),
            Some(Ok(ThemeCommand::Update { name })) => {
                let name = name.map_or_else(|| self.theme_setting.clone(), str::to_string);
                self.update_theme(&name, now)
            }
            _ => Err(Msg::Label(Label::PaletteThemeUsage)),
        };
        match done {
            Ok(()) => self.cmdline.close(),
            Err(msg) => self.cmdline.error = Some(msg),
        }
    }

    /// The first free name for a copy of `from`: `<from>-copy`, `<from>-copy-2`, …
    pub(crate) fn copy_name(&self, from: &str) -> String {
        authoring::free_copy_name(self.prefs.theme_dir.as_deref(), from)
    }

    /// Copy theme `from` to the new theme file `name`, then draw with it and save it.
    pub(crate) fn copy_theme(&mut self, from: &str, name: &str, now: Instant) -> Result<(), Msg> {
        let dir = self.prefs.theme_dir.clone().ok_or(Msg::Label(Label::ThemeFileNoFolder))?;
        let written = authoring::copy(&dir, from, name, self.look);
        let written = written.map_err(|e| match e {
            AuthorError::Source(e) => {
                Msg::ThemeFileCannotCopy { name: shown(from), why: e.message(&self.i18n).to_string() }
            }
            e => self.author_problem(&e),
        })?;
        self.use_written(name, &written, now)
    }

    /// `:theme new <name>`: the new theme file `name`, then drawn with and saved.
    fn new_theme(&mut self, name: &str, now: Instant) -> Result<(), Msg> {
        let dir = self.prefs.theme_dir.clone().ok_or(Msg::Label(Label::ThemeFileNoFolder))?;
        let written = authoring::new(&dir, name, &self.new_base(), self.look).map_err(|e| self.author_problem(&e))?;
        self.use_written(name, &written, now)
    }

    /// The theme file `name` just written at `path`: drawn with and saved (through the config
    /// file's own rules: a file that cannot be written is not), and where it is said.
    fn use_written(&mut self, name: &str, path: &Path, now: Instant) -> Result<(), Msg> {
        self.set_theme(name, now)?;
        self.info(Msg::ThemeFileWritten { name: name.to_string(), path: path.display().to_string() }, now);
        Ok(())
    }

    /// `:theme update [name]`: the tokens theme file `name` does not list added, and said.
    fn update_theme(&mut self, name: &str, now: Instant) -> Result<(), Msg> {
        let dir = self.prefs.theme_dir.clone().ok_or(Msg::Label(Label::ThemeFileNoFolder))?;
        let msg = match authoring::update(&dir, name, self.look).map_err(|e| self.author_problem(&e))? {
            Updated::Added { path, count } => {
                Msg::ThemeFileUpdated { path: path.display().to_string(), count: count as u64 }
            }
            Updated::Complete { path } => Msg::ThemeFileComplete { path: path.display().to_string() },
        };
        self.info(msg, now);
        Ok(())
    }

    /// The words for why a theme file was not written.
    fn author_problem(&self, e: &AuthorError) -> Msg {
        let p = |path: &Path| path.display().to_string();
        match e {
            AuthorError::Source(e) => self.theme_problem(e),
            AuthorError::Name(e) => e.msg(),
            AuthorError::Unwritable { token } => Msg::ThemeFileUnwritable { token: (*token).to_string() },
            AuthorError::Exists(path) => Msg::ThemeFileExists { path: p(path) },
            AuthorError::Write(path) => Msg::ThemeFileWriteFailed { path: p(path) },
            AuthorError::Shared(path) => Msg::ThemeFileShared { path: p(path) },
            AuthorError::NotAFile(name) => Msg::ThemeFileNotAFile { name: shown(name) },
            AuthorError::Changed(path) => Msg::ThemeFileChanged { path: p(path) },
        }
    }
}

/// A name typed, safe to print on one line.
fn shown(text: &str) -> String {
    Remote::new(text).line().into_string()
}
