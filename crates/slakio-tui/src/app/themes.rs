//! Which theme the app draws with: the setting read at launch, and a theme named while it runs
//! (a built-in one or a theme file). A theme file that cannot be used never stops the app: at
//! launch it draws with `terminal` and the status line says why, the setting kept as written.
//! On the settings screen's theme row a theme is shown before it is kept: the theme drawn before
//! comes back with `Esc`, and one that cannot be used is never kept (why is said on top). Showing
//! a theme changes colours only: pictures are never sent to the terminal again for it.

use crate::app::App;
use crate::theme::{self, Look, Theme};
use slakio_core::config::Config;
use slakio_core::i18n::Msg;
use slakio_core::theme_file::{self, ThemeError};
use std::time::Instant;

/// A theme shown on the settings screen and not kept yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    /// Its place in the theme row's names.
    pub index: usize,
    /// The theme drawn before the first one shown.
    pub before: Theme,
    /// Why it cannot be used (the theme before stays drawn).
    pub error: Option<Msg>,
}

impl App {
    /// At launch: keep the config file's settings `cfg` (`writable`: the file may be written),
    /// and draw with its theme on a terminal `look`, theme files read from `themes/` next to the
    /// file. A theme that cannot be used draws `terminal` instead and the status line says why;
    /// the setting stays as it is (nothing is written).
    pub fn load_at_start(&mut self, look: Look, cfg: &Config, writable: bool, now: Instant) {
        self.look = look;
        self.prefs.theme_dir = cfg.path.as_deref().and_then(theme_file::dir);
        self.prefs.config = cfg.clone();
        self.prefs.writable = writable;
        self.prefs.download_env = std::env::var(super::settings::DOWNLOAD_ENV).ok().filter(|v| !v.is_empty());
        let name = cfg.theme.as_str();
        self.theme_setting = name.to_string();
        match self.theme_named(name) {
            Ok(t) => self.theme = t,
            Err(e) => {
                self.theme = look.theme("terminal");
                let why = self.theme_problem(&e);
                self.warn(why, now);
            }
        }
    }

    /// The theme `name` (built-in, `auto` or a theme file) as it is drawn here, without changing
    /// what is drawn: the theme file is read again each time.
    pub fn theme_named(&self, name: &str) -> Result<Theme, ThemeError> {
        theme::load(name, self.look, self.prefs.theme_dir.as_deref())
    }

    /// The status line's words for a theme that cannot be used: `Theme not used: <why>`.
    pub fn theme_problem(&self, e: &ThemeError) -> Msg {
        Msg::ThemeNotUsed { why: e.message(&self.i18n).to_string() }
    }

    /// `name` (typed after `:theme`) is the name of a theme file that is there (it may still not
    /// be usable), not of a built-in theme.
    pub(crate) fn theme_file_there(&self, name: &str) -> bool {
        let Some(dir) = &self.prefs.theme_dir else { return false };
        !theme::NAMES.contains(&name.to_ascii_lowercase().as_str())
            && theme_file::valid_name(name)
            && theme_file::path(dir, name).symlink_metadata().is_ok()
    }

    /// The names of the theme files there are (none without a themes folder), sorted.
    pub fn theme_files(&self) -> Vec<String> {
        self.prefs.theme_dir.as_deref().map(|d| theme_file::names(d).names).unwrap_or_default()
    }

    /// `h`/`l` on the theme row: draw with the theme before or after the one shown, not saved.
    pub(super) fn preview_theme(&mut self, step: isize) {
        let Some(screen) = self.prefs.screen.as_ref() else { return };
        let n = screen.themes.len().max(1) as isize;
        let at = match &screen.preview {
            Some(p) => p.index,
            None => screen.themes.iter().position(|t| *t == self.theme_setting).unwrap_or(0),
        } as isize;
        let index = (at + step).rem_euclid(n) as usize;
        let Some(name) = screen.themes.get(index).cloned() else { return };
        let before = screen.preview.as_ref().map_or_else(|| self.theme.clone(), |p| p.before.clone());
        let error = match self.theme_named(&name) {
            Ok(t) => {
                self.theme = t;
                None
            }
            Err(e) => {
                self.theme = before.clone();
                Some(self.theme_problem(&e))
            }
        };
        if let Some(s) = self.prefs.screen.as_mut() {
            s.preview = Some(Preview { index, before, error });
        }
    }

    /// `Enter` on the theme row: keep the theme shown (saved); one that cannot be used stays
    /// shown, not kept.
    pub(super) fn keep_preview(&mut self, now: Instant) {
        let Some(s) = self.prefs.screen.as_mut() else { return };
        let Some(p) = s.preview.take_if(|p| p.error.is_none()) else { return };
        let name = s.themes.get(p.index).cloned().unwrap_or_default();
        match self.set_theme(&name, now) {
            Ok(()) => {
                if let Some(s) = self.prefs.screen.as_mut() {
                    s.problem = None;
                }
            }
            Err(msg) => {
                self.theme = p.before;
                self.warn(msg, now);
            }
        }
    }

    /// The settings screen closes: a theme only shown goes back to the one before.
    pub(super) fn end_preview(&mut self) {
        if let Some(p) = self.prefs.screen.as_mut().and_then(|s| s.preview.take()) {
            self.theme = p.before;
        }
    }
}
