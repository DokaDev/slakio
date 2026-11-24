//! The settings screen (`Space ,`, `:settings`): every setting of the table ([`table`]) under its
//! group, with its value. `j`/`k` move (round again past the ends), `h`/`l` (also `Enter` and
//! `Space`) change the selected setting at once and save it in the config file through the same
//! path as `:set`, so the screen, `:set` and the file always agree; `Esc` or `q` close. On the
//! theme row `h`/`l` only show a theme ([`super::themes`]): `Enter` keeps it, `Esc` goes back. On
//! the downloads row `Enter` (also `h`/`l`) types a folder; `c` on the theme row copies the theme shown to a new
//! theme file, its name typed first. A setting that applies on the next launch says so.
//! It is modal and takes the keys only (no mouse).
//!
//! The config file's settings are kept here as read at launch ([`Prefs`]): those the app does not
//! use while it runs (`language`, `network`, …) are shown and saved from there.

pub mod table;

use super::themes::Preview;
use super::{App, Effect, Guide};
use crate::keymap::KeyChord;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use slakio_core::config::{self, Config};
use slakio_core::i18n::{Label, Msg};
use std::path::PathBuf;
use std::time::Instant;
use table::{SETTINGS, Values};

/// The environment variable that wins over the `downloads` setting.
pub const DOWNLOAD_ENV: &str = "SLAKIO_DOWNLOAD_DIR";
/// The environment variable that switches the network off whatever the `network` setting (the
/// request governor's; the UI does not name the network crate).
pub const NETWORK_ENV: &str = "SLAKIO_NETWORK";

/// The config file's settings and the settings screen.
#[derive(Clone, Debug, Default)]
pub struct Prefs {
    /// The settings as the config file has them (as changed here since).
    pub config: Config,
    /// The config file can be written (a file with errors is never written over).
    pub writable: bool,
    /// Where theme files are read from (`themes/` next to the config file; none without one).
    pub theme_dir: Option<PathBuf>,
    /// `SLAKIO_DOWNLOAD_DIR`, when set: the downloads row only shows it.
    pub download_env: Option<String>,
    /// `SLAKIO_NETWORK`, when it switches the network off: the network row only shows it.
    pub network_env: Option<String>,
    /// The settings screen, while it is open.
    pub screen: Option<Screen>,
}

/// The settings screen's state.
#[derive(Clone, Debug, Default)]
pub struct Screen {
    /// The selected row (an index into [`SETTINGS`]).
    pub selected: usize,
    /// The theme row's names: the built-in ones, then the theme files there when it opened.
    pub themes: Vec<String>,
    /// The theme shown but not kept, while one is.
    pub preview: Option<Preview>,
    /// Why the theme set is not used (said on top), when it cannot be.
    pub problem: Option<Msg>,
    /// The downloads folder being typed, and why the last one typed cannot be used.
    pub folder: Option<(String, Option<Msg>)>,
    /// `c` on the theme row: the name of the copy being typed, and why the last one cannot be.
    pub name: Option<(String, Option<Msg>)>,
}

impl App {
    /// `Space ,` / `:settings`: what is open over the screen closes, the settings screen opens.
    pub(super) fn open_settings(&mut self) {
        self.cmdline.close();
        self.help = None;
        self.switcher = None;
        self.keys.clear();
        self.guide = Guide::default();
        // Keyboard only: a drag going on stops here (its button-up never reaches it).
        self.tab_drag = None;
        self.seam_drag = None;
        let mut themes: Vec<String> = config::THEMES.iter().map(|t| (*t).to_string()).collect();
        themes.extend(self.theme_files());
        let problem = self.theme_named(&self.theme_setting).err().map(|e| self.theme_problem(&e));
        self.prefs.screen = Some(Screen { themes, problem, ..Screen::default() });
    }

    /// Characters typed now are text: a text input has the keyboard, or the settings screen's
    /// folder or name input is open (Hangul stays Hangul there).
    pub fn typing_text(&self) -> bool {
        self.key_context().is_text_input()
            || (self.key_context() == crate::keymap::Ctx::Settings
                && self.prefs.screen.as_ref().is_some_and(|s| s.folder.is_some() || s.name.is_some()))
    }

    /// The settings screen closes; a theme only shown goes back to the one before.
    fn close_settings(&mut self) {
        self.end_preview();
        self.prefs.screen = None;
    }

    /// The value setting `k` has now, as `:set` writes it.
    pub fn setting_value(&self, k: usize) -> String {
        let s = self.settings;
        let c = &self.prefs.config;
        let value = match SETTINGS[k].key {
            "theme" => return self.theme_setting.clone(),
            "density" if s.compact => "compact",
            "density" => "comfortable",
            "nav_rows" if self.shell.nav_folded => "collapsed",
            "nav_rows" => "expanded",
            "icons" if s.icons => "on",
            "icons" => "off",
            "avatars" if !s.avatars => "off",
            "avatars" if s.photos => "image",
            "avatars" => "initials",
            "images" if self.images.setting.is_empty() => "auto",
            "images" => &self.images.setting,
            "language" => &c.language,
            "downloads" => &c.downloads,
            "download_max_mb" => return c.download_max_mb.to_string(),
            "network" => &c.network,
            "live" => &c.live,
            "restore" => &c.restore,
            "cache" => &c.cache,
            other => unreachable!("setting {other} has no value here"),
        };
        value.to_string()
    }

    /// Set setting `k` to `value` (checked by [`table::check`]) and save it: what the settings
    /// screen and `:set` do. Why it cannot be, else.
    pub fn apply_setting(&mut self, k: usize, value: &str, now: Instant) -> Result<(), Msg> {
        let value = table::check(k, value)?;
        let key = SETTINGS[k].key;
        match key {
            "theme" => return self.set_theme(&value, now),
            "density" => return self.set_density(&value, now),
            "icons" => return self.set_icons(&value, now),
            "avatars" => return self.set_avatars(&value, now),
            "images" => {
                self.set_images(&value, now);
                return Ok(());
            }
            "nav_rows" => {
                self.fold_views(value == "collapsed");
                self.effects.push(Effect::Save { key, value: value.clone() });
                self.info(Msg::NavRowsChanged { name: value }, now);
                return Ok(());
            }
            "downloads" => return self.set_downloads(&value, now),
            "download_max_mb" => {
                let mb = value.parse::<u32>().unwrap_or(self.prefs.config.download_max_mb);
                self.prefs.config.download_max_mb = mb;
                self.downloads.max = u64::from(mb) * 1_000_000;
            }
            "language" => self.prefs.config.language.clone_from(&value),
            "network" => match &self.prefs.network_env {
                Some(env) => return Err(Msg::SettingsNetworkEnv { env: format!("{NETWORK_ENV}={env}") }),
                None => self.prefs.config.network.clone_from(&value),
            },
            "live" => self.prefs.config.live.clone_from(&value),
            "restore" => self.prefs.config.restore.clone_from(&value),
            "cache" => self.prefs.config.cache.clone_from(&value),
            other => unreachable!("setting {other} cannot be set here"),
        }
        self.effects.push(Effect::Save { key, value: value.clone() });
        let (key, value) = (key.to_string(), value);
        self.info(
            if SETTINGS[k].restart && !self.prefs.writable {
                Msg::SettingsChangedUnsaved { key, value }
            } else if SETTINGS[k].restart {
                Msg::SettingsChangedRestart { key, value }
            } else {
                Msg::SettingsChanged { key, value }
            },
            now,
        );
        Ok(())
    }

    /// The downloads folder set to `value` (empty: the default): an absolute path or one in the
    /// home folder (`~/…`), never one that depends on where slakio was started; used from the next
    /// download (the demo keeps its own folder).
    fn set_downloads(&mut self, value: &str, now: Instant) -> Result<(), Msg> {
        if let Some(env) = &self.prefs.download_env {
            return Err(Msg::SettingsDownloadsEnv { env: format!("{DOWNLOAD_ENV}={env}") });
        }
        // The rule the launch uses, without the environment.
        if slakio_core::paths::downloads(value, |k| (k == "HOME").then(|| "~".to_string())).is_none() {
            return Err(Msg::SettingsDownloadsRelative { path: value.to_string() });
        }
        if !self.backend.is_some_and(|c| c.demo) {
            let home = self.downloads.home.as_ref().map(|h| h.to_string_lossy().into_owned());
            self.downloads.dir =
                slakio_core::paths::downloads(value, |k| if k == "HOME" { home.clone() } else { None });
        }
        self.prefs.config.downloads = value.to_string();
        self.effects.push(Effect::Save { key: "downloads", value: value.to_string() });
        self.info(Msg::SettingsChanged { key: "downloads".to_string(), value: value.to_string() }, now);
        Ok(())
    }

    /// A key on the settings screen (see the module's text).
    pub(super) fn settings_key(&mut self, key: KeyChord, now: Instant) {
        // Too small to be drawn (as the screen under it): no change unseen, it only closes.
        if self.sign_in().is_none() && self.backend.is_some() && crate::screen::too_small(self.size) {
            let close = key.mods.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
            if close || matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                self.close_settings();
            }
            return;
        }
        let Some(s) = self.prefs.screen.as_mut() else { return };
        if s.folder.is_some() {
            return self.folder_key(key, now);
        }
        if s.name.is_some() {
            return self.name_key(key, now);
        }
        let ctrl = key.mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let n = SETTINGS.len();
        let step: isize = match key.code {
            KeyCode::Char('c') if ctrl => return self.close_settings(),
            _ if ctrl => return,
            KeyCode::Char('j') | KeyCode::Down => {
                s.selected = (s.selected + 1) % n;
                return;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                s.selected = (s.selected + n - 1) % n;
                return;
            }
            KeyCode::Esc | KeyCode::Char('q') => return self.close_settings(),
            KeyCode::Char('h') | KeyCode::Left => -1,
            KeyCode::Char('l') | KeyCode::Right => 1,
            KeyCode::Enter | KeyCode::Char(' ') => 0,
            KeyCode::Char('c') if SETTINGS[s.selected].values == Values::Themes => return self.open_name(),
            _ => return,
        };
        let k = s.selected;
        match SETTINGS[k].values {
            Values::Themes if step == 0 => self.keep_preview(now),
            Values::Themes => self.preview_theme(step),
            Values::Folder => self.open_folder(now),
            Values::Fixed(values) => {
                let at = values.iter().position(|(v, _)| *v == self.setting_value(k)).unwrap_or(0) as isize;
                let next = (at + if step == 0 { 1 } else { step }).rem_euclid(values.len() as isize);
                self.change(k, values[next as usize].0, now);
            }
            Values::Megabytes(presets) => {
                let mb = self.prefs.config.download_max_mb;
                let next = if step < 0 {
                    presets.iter().rev().find(|p| **p < mb).or(presets.last())
                } else {
                    presets.iter().find(|p| **p > mb).or(presets.first())
                };
                if let Some(next) = next {
                    self.change(k, &next.to_string(), now);
                }
            }
        }
    }

    /// Setting `k` set to `value` from the screen; why not, in the status line.
    fn change(&mut self, k: usize, value: &str, now: Instant) {
        if let Err(msg) = self.apply_setting(k, value, now) {
            self.warn(msg, now);
        }
    }

    /// `Enter` on the downloads row: type the folder, from the one set (said why not where
    /// `SLAKIO_DOWNLOAD_DIR` sets it).
    fn open_folder(&mut self, now: Instant) {
        if let Some(env) = &self.prefs.download_env {
            let msg = Msg::SettingsDownloadsEnv { env: format!("{DOWNLOAD_ENV}={env}") };
            return self.info(msg, now);
        }
        let text = self.prefs.config.downloads.clone();
        if let Some(s) = self.prefs.screen.as_mut() {
            s.folder = Some((text, None));
        }
    }

    /// A key while the folder is typed: `Enter` uses it (or says why not), `Esc` leaves it.
    fn folder_key(&mut self, key: KeyChord, now: Instant) {
        let Some((text, error)) = self.prefs.screen.as_mut().and_then(|s| s.folder.as_mut()) else { return };
        let ctrl = key.mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Char('u') if ctrl => {
                text.clear();
                *error = None;
                return;
            }
            KeyCode::Char('c') if ctrl => {}
            KeyCode::Enter => {
                let typed = text.trim().to_string();
                match self.set_downloads(&typed, now) {
                    Ok(()) => {}
                    Err(msg) => {
                        if let Some((_, e)) = self.prefs.screen.as_mut().and_then(|s| s.folder.as_mut()) {
                            *e = Some(msg);
                        }
                        return;
                    }
                }
            }
            KeyCode::Backspace => {
                text.pop();
                *error = None;
                return;
            }
            KeyCode::Char(c) if !ctrl => {
                text.push(c);
                *error = None;
                return;
            }
            _ => return,
        }
        if let Some(s) = self.prefs.screen.as_mut() {
            s.folder = None;
        }
    }

    /// What the settings screen shows for setting `k`: its value and what it means.
    pub fn setting_shown(&self, k: usize) -> (String, String) {
        let spec = &SETTINGS[k];
        let label = |l: Label| self.i18n.label(l).to_string();
        let value = self.setting_value(k);
        match spec.values {
            Values::Themes => {
                let name = self
                    .prefs
                    .screen
                    .as_ref()
                    .and_then(|s| s.preview.as_ref().and_then(|p| s.themes.get(p.index)))
                    .cloned()
                    .unwrap_or(value);
                // A theme file that cannot be used says so, shown or set.
                let broken = self.prefs.screen.as_ref().is_some_and(|s| match &s.preview {
                    Some(p) => p.error.is_some(),
                    None => s.problem.is_some(),
                });
                let what = if config::THEMES.contains(&name.as_str()) {
                    super::palette::theme_label(&name)
                } else if broken {
                    Label::SettingsThemeFileBroken
                } else {
                    Label::SettingsThemeFile
                };
                (name, label(what))
            }
            Values::Folder => match &self.prefs.download_env {
                Some(env) => (
                    env.clone(),
                    self.i18n.msg(&Msg::SettingsDownloadsEnv { env: DOWNLOAD_ENV.to_string() }).to_string(),
                ),
                None if value.is_empty() => ("~/Downloads/slakio".to_string(), label(Label::SettingsDownloadsEdit)),
                None => (value, label(Label::SettingsDownloadsEdit)),
            },
            Values::Megabytes(presets) if !presets.iter().any(|p| p.to_string() == value) => {
                (self.i18n.msg(&Msg::SettingsCustom { value }).to_string(), label(Label::SettingsMaxMb))
            }
            Values::Megabytes(_) => (value, label(Label::SettingsMaxMb)),
            // `SLAKIO_NETWORK=off` wins for this run, whatever is saved.
            Values::Fixed(_) if spec.key == "network" && self.prefs.network_env.is_some() => {
                let env = format!("{NETWORK_ENV}={}", self.prefs.network_env.as_deref().unwrap_or_default());
                ("off".to_string(), self.i18n.msg(&Msg::SettingsNetworkEnv { env }).to_string())
            }
            // A file with errors is never written: the next launch does not see the change.
            Values::Fixed(_) if spec.restart && !self.prefs.writable => (value, label(Label::SettingsOnRestartUnsaved)),
            Values::Fixed(_) if spec.restart => (value, label(Label::SettingsOnRestart)),
            Values::Fixed(values) => {
                let what = values.iter().find(|(v, _)| *v == value).and_then(|(_, l)| *l);
                (value, what.map(label).unwrap_or_default())
            }
        }
    }

    /// The config file settings are saved in, if there is one.
    pub fn config_file(&self) -> Option<&std::path::Path> {
        self.prefs.config.path.as_deref()
    }
}

#[cfg(test)]
mod tests;
