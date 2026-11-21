//! Which theme the app draws with: the setting read at launch, and a theme named while it runs
//! (a built-in one or a theme file). A theme file that cannot be used never stops the app: at
//! launch it draws with `terminal` and the status line says why, the setting kept as written.

use crate::app::App;
use crate::theme::{self, Look, Theme};
use slakio_core::i18n::Msg;
use slakio_core::theme_file::{self, ThemeError};
use std::path::Path;
use std::time::Instant;

impl App {
    /// At launch: draw with the setting `name` on a terminal `look`, theme files read from
    /// `themes/` next to the config file `config`. A theme that cannot be used draws `terminal`
    /// instead and the status line says why; `name` stays the setting (nothing is written).
    pub fn load_at_start(&mut self, look: Look, name: &str, config: Option<&Path>, now: Instant) {
        self.look = look;
        self.theme_dir = config.and_then(theme_file::dir);
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
        theme::load(name, self.look, self.theme_dir.as_deref())
    }

    /// The status line's words for a theme that cannot be used: `Theme not used: <why>`.
    pub fn theme_problem(&self, e: &ThemeError) -> Msg {
        Msg::ThemeNotUsed { why: e.message(&self.i18n).to_string() }
    }

    /// `name` (typed after `:theme`) is the name of a theme file that is there (it may still not
    /// be usable), not of a built-in theme.
    pub(crate) fn theme_file_there(&self, name: &str) -> bool {
        let Some(dir) = &self.theme_dir else { return false };
        !theme::NAMES.contains(&name.to_ascii_lowercase().as_str())
            && theme_file::valid_name(name)
            && theme_file::path(dir, name).symlink_metadata().is_ok()
    }
}
