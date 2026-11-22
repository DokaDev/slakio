//! The words for why a theme or a name cannot be used (en and ko catalog entries).

use super::{NameError, Problem, ThemeError};
use crate::i18n::{I18n, Label, Localized, Msg};
use crate::sanitize::Remote;
use std::path::Path;

impl ThemeError {
    /// The words for the status line and the settings screen, as `path:line: problem` for a
    /// problem in a file.
    pub fn message(&self, i18n: &I18n) -> Localized {
        let p = |path: &Path| shown(&path.display().to_string());
        let m = match self {
            ThemeError::BadName(name) => Msg::ThemeFileBadName { name: shown(name) },
            ThemeError::Unknown { name, dir: Some(dir) } => {
                Msg::ThemeFileMissing { name: shown(name), path: p(&dir.join(format!("{name}.toml"))) }
            }
            ThemeError::Unknown { name, dir: None } => Msg::ThemeFileNoDir { name: shown(name) },
            ThemeError::Shadows { path } => Msg::ThemeFileShadows { path: p(path) },
            ThemeError::Symlink { path } => Msg::ThemeFileSymlink { path: p(path) },
            ThemeError::NotPlain { path } => Msg::ThemeFileNotPlain { path: p(path) },
            ThemeError::TooLarge { path } => Msg::ThemeFileTooLarge { path: p(path) },
            ThemeError::Read { path, .. } => Msg::ThemeFileReadFailed { path: p(path) },
            ThemeError::File { path, line: Some(line), problem } => {
                Msg::ThemeFileAtLine { path: p(path), line: line.to_string(), problem: problem_words(problem, i18n) }
            }
            ThemeError::File { path, line: None, problem } => {
                Msg::ThemeFileInFile { path: p(path), problem: problem_words(problem, i18n) }
            }
        };
        i18n.msg(&m)
    }
}

/// The words for a problem in a theme file.
fn problem_words(problem: &Problem, i18n: &I18n) -> String {
    let m = match problem {
        Problem::Toml(words) => return shown(words),
        Problem::NotText => Msg::Label(Label::ThemeFileNotText),
        Problem::UnknownToken(name) => Msg::ThemeFileUnknownToken { name: shown(name) },
        Problem::NotAColor(name) => Msg::ThemeFileNotAColor { name: shown(name) },
        Problem::NotAStyle(name) => Msg::ThemeFileNotAStyle { name: shown(name) },
        Problem::BadColor(value) => Msg::ThemeFileBadColor { value: shown(value) },
        Problem::BadModifier(value) => Msg::ThemeFileBadModifier { value: shown(value) },
        Problem::UnknownBase(name) => Msg::ThemeFileUnknownBase { name: shown(name) },
        Problem::BadPalette(name) => Msg::ThemeFileBadPalette { name: shown(name) },
        Problem::BadPercent(name) => Msg::ThemeFileBadPercent { name: shown(name) },
        Problem::BadDim => Msg::Label(Label::ThemeFileBadDim),
    };
    i18n.msg(&m).to_string()
}

/// Text from a file or a name typed, safe to print on one line.
fn shown(text: &str) -> String {
    Remote::new(text).line().into_string()
}

impl NameError {
    /// The words for the status line.
    pub fn message(&self, i18n: &I18n) -> Localized {
        i18n.msg(&self.msg())
    }

    /// The message of [`Self::message`], to be said later.
    pub fn msg(&self) -> Msg {
        match self {
            NameError::Invalid(name) => Msg::ThemeFileBadName { name: shown(name) },
            NameError::Builtin(name) => Msg::ThemeFileBuiltinName { name: shown(name) },
            NameError::Reserved(name) => Msg::ThemeFileReservedName { name: shown(name) },
        }
    }
}
