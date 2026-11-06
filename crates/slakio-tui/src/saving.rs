//! Where the binary's loop saves what the app asks it to: the config file, the tabs and panes
//! for the next run, and the error log for a save that fails.

use slakio_core::config;
use slakio_core::fault::ErrorLog;
use slakio_core::i18n::{Label, Msg};
use slakio_tui::app::App;
use slakio_tui::layout_file::LayoutFile;
use std::time::Instant;

pub(crate) struct Saved {
    pub config: Option<std::path::PathBuf>,
    pub errors: Option<std::path::PathBuf>,
    /// The tabs and panes, kept for the next run.
    pub layout: LayoutFile,
}

impl Saved {
    /// Keep the layout as it is now (`end`: the run ends, so whatever waits is written).
    pub fn keep_layout(&mut self, app: &App, end: bool) {
        // Looked at only when the layout may have changed (not on every wake of the loop).
        let result = self.layout.keep(app.layout_stamp(), || app.layout_to_save(), Instant::now(), end);
        if let Err(fault) = result {
            ErrorLog::new(self.errors.clone()).record("layout", &fault);
        }
    }
}

impl Saved {
    /// Open `url` in the browser; a failure is said and goes to the error log.
    pub fn open(&self, url: &str, app: &mut App) {
        if let Err(e) = slakio_tui::opener::open(url) {
            ErrorLog::new(self.errors.clone()).record("open", &slakio_core::fault::Fault::io(&e));
            app.warn(Msg::Label(Label::OpenFailed), Instant::now());
        }
    }
}

impl Saved {
    /// Save setting `key` as `value` in the config file; `true` when that failed (the app says so).
    pub fn save(&self, key: &str, value: &str, app: &mut App) -> bool {
        let result = match &self.config {
            Some(path) => config::set(path, key, value),
            None => Err(slakio_core::fault::Fault::other("no config directory")),
        };
        let Err(fault) = result else { return false };
        ErrorLog::new(self.errors.clone()).record("config", &fault);
        app.warn(Msg::Label(Label::ConfigSaveFailed), Instant::now());
        true
    }
}
