//! The settings the palette changes and saves in the config file: `:theme`, `:avatars`,
//! `:images`, `:density`, `:icons` (each with a value, or alone: the other value).

use super::{DENSITY_VALUES, ICON_VALUES, NET_VALUES, Row, icons_arg, setting_arg};
use crate::action::{self, AppAction};
use crate::app::todos::{FILTER_VALUES, SORT_VALUES};
use crate::app::{App, Effect};
use crate::images::Chosen;
use crate::images::detect::IMAGES_ENV;
use crate::theme::{self, NAMES};
use slakio_core::i18n::{Label, Msg};
use std::time::Instant;

impl App {
    /// `:theme <name>`: draw with theme `name` from now on and save it in the config file. A
    /// name the setting does not take changes nothing and says which ones it takes.
    pub fn set_theme(&mut self, name: &str, now: Instant) -> Result<(), Msg> {
        let name = name.trim().to_ascii_lowercase();
        if !theme::NAMES.contains(&name.as_str()) {
            return Err(Msg::ThemeUnknown { name, names: theme::NAMES.join(", ") });
        }
        self.theme = self.look.theme(&name);
        self.theme_setting.clone_from(&name);
        self.effects.push(Effect::Save { key: "theme", value: name.clone() });
        self.info(Msg::ThemeChanged { name }, now);
        Ok(())
    }

    /// `:avatars <initials|image|off>`: picture people by their initials chip, their profile
    /// photo (where pictures are drawn; initials elsewhere), or not, from now on, and save it in
    /// the config file. Another value changes nothing and says which ones work.
    pub fn set_avatars(&mut self, value: &str, now: Instant) -> Result<(), Msg> {
        let value = value.trim().to_ascii_lowercase();
        if !AVATAR_VALUES.contains(&value.as_str()) {
            return Err(Msg::AvatarsUnknown { name: value, names: AVATAR_VALUES.join(", ") });
        }
        self.settings.avatars = value != "off";
        self.settings.photos = value == "image";
        if !self.settings.photos {
            self.images.forget_photos();
        }
        self.effects.push(Effect::Save { key: "avatars", value: value.clone() });
        self.info(Msg::AvatarsChanged { name: value }, now);
        Ok(())
    }

    /// `:avatars`, `:density`, `:icons` alone: the other value of the setting, saved; `:net`
    /// alone: the network the other way.
    pub(in crate::app) fn toggle(&mut self, a: AppAction, now: Instant) {
        let s = self.settings;
        let done = match a {
            AppAction::ToggleAvatars => self.set_avatars(if s.avatars { "off" } else { "initials" }, now),
            AppAction::ToggleImages => {
                return self.set_images(if self.images.on { "off" } else { "auto" }, now);
            }
            AppAction::ToggleDensity => self.set_density(if s.compact { "comfortable" } else { "compact" }, now),
            AppAction::ToggleNetwork => return self.set_net(self.offline(), now),
            AppAction::SignInAgain => return self.sign_in_again(),
            _ => self.set_icons(if s.icons { "off" } else { "on" }, now),
        };
        if let Err(msg) = done {
            self.warn(msg, now);
        }
    }

    /// `:density <comfortable|compact>`: lay messages out so from now on, and save it in the
    /// config file. Another value changes nothing and says which ones work.
    pub fn set_density(&mut self, value: &str, now: Instant) -> Result<(), Msg> {
        let value = value.trim().to_ascii_lowercase();
        if !DENSITY_VALUES.contains(&value.as_str()) {
            return Err(Msg::DensityUnknown { name: value, names: DENSITY_VALUES.join(", ") });
        }
        self.settings.compact = value == "compact";
        self.effects.push(Effect::Save { key: "density", value: value.clone() });
        self.info(Msg::DensityChanged { name: value }, now);
        Ok(())
    }

    /// `:icons <on|off>`: draw Nerd Font icons, or text instead, from now on, and save it in the
    /// config file. Another value changes nothing and says which ones work.
    pub fn set_icons(&mut self, value: &str, now: Instant) -> Result<(), Msg> {
        let value = value.trim().to_ascii_lowercase();
        if !ICON_VALUES.contains(&value.as_str()) {
            return Err(Msg::IconsUnknown { name: value, names: ICON_VALUES.join(", ") });
        }
        self.settings.icons = value == "on";
        self.effects.push(Effect::Save { key: "icons", value: value.clone() });
        self.info(Msg::IconsChanged { name: value }, now);
        Ok(())
    }
}

impl App {
    /// `:images <auto|on|off>`: draw pictures where the terminal surely shows them, always, or
    /// never, from now on (turned off, every picture leaves the screen at once), and save it in
    /// the config file. Said: a choice the environment overrides for this run, and pictures on
    /// where slakio cannot tell that the terminal shows them. Another value changes nothing and
    /// says which ones work.
    pub fn set_images(&mut self, value: &str, now: Instant) {
        let value = value.trim().to_ascii_lowercase();
        if !IMAGE_VALUES.contains(&value.as_str()) {
            let names = IMAGE_VALUES.join(", ");
            return self.warn(Msg::ImagesUnknown { name: value, names }, now);
        }
        self.effects.push(Effect::Save { key: "images", value: value.clone() });
        match self.images.choose(&value) {
            Chosen::Done => self.info(Msg::ImagesChanged { name: value }, now),
            Chosen::Forced => self.warn(Msg::Label(Label::ImagesForced), now),
            Chosen::Pinned(pinned) => {
                self.warn(Msg::ImagesPinned { name: value, env: IMAGES_ENV.to_string(), value: pinned }, now);
            }
        }
    }

    /// The palette's row of `:images` value `i`.
    pub(super) fn images_row(&self, i: usize) -> Row {
        let words = match IMAGE_VALUES[i] {
            "auto" => Label::ImagesAuto,
            "on" => Label::ImagesOn,
            _ => Label::ImagesOff,
        };
        let current = IMAGE_VALUES[i] == self.images.setting;
        Row {
            name: IMAGE_VALUES[i].to_string(),
            label: self.i18n.label(words).to_string(),
            keys: if current { self.i18n.label(Label::PaletteCurrent).to_string() } else { String::new() },
        }
    }
}

/// The values `:images` takes.
pub const IMAGE_VALUES: &[&str] = &["auto", "on", "off"];

/// The values `:avatars` takes.
pub const AVATAR_VALUES: &[&str] = &["initials", "image", "off"];

/// The value of `:avatars <value>` (also `:set avatars=<value>`).
pub fn avatars_arg(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let (word, rest) = line.split_once(char::is_whitespace)?;
    match word {
        "avatars" => Some(rest.trim()),
        "set" => rest.trim().strip_prefix("avatars=").map(str::trim),
        _ => None,
    }
}

/// The theme name of `:theme <name>` (also `:colorscheme`, `:colo` as in vim, and
/// `:set theme=<name>`).
pub fn theme_arg(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let (word, rest) = line.split_once(char::is_whitespace)?;
    match word {
        "theme" | "colorscheme" | "colo" => Some(rest.trim()),
        "set" => rest.trim().strip_prefix("theme=").map(str::trim),
        _ => None,
    }
}

/// Why `text` (a command line that matches no entry) runs nothing.
pub(super) fn unknown(text: String) -> Msg {
    match (theme_arg(&text), avatars_arg(&text), icons_arg(&text)) {
        (Some(name), ..) => Msg::ThemeUnknown { name: name.to_string(), names: NAMES.join(", ") },
        (_, Some(name), _) => Msg::AvatarsUnknown { name: name.to_string(), names: AVATAR_VALUES.join(", ") },
        (.., Some(name)) => Msg::IconsUnknown { name: name.to_string(), names: ICON_VALUES.join(", ") },
        _ if setting_arg(&text, "images").is_some() => Msg::ImagesUnknown {
            name: setting_arg(&text, "images").unwrap_or_default().to_string(),
            names: IMAGE_VALUES.join(", "),
        },
        _ if setting_arg(&text, "density").is_some() => Msg::DensityUnknown {
            name: setting_arg(&text, "density").unwrap_or_default().to_string(),
            names: DENSITY_VALUES.join(", "),
        },
        _ if setting_arg(&text, "filter").is_some() => Msg::FilterUnknown {
            name: setting_arg(&text, "filter").unwrap_or_default().to_string(),
            names: FILTER_VALUES.join(", "),
        },
        _ if setting_arg(&text, "sort").is_some() => Msg::SortUnknown {
            name: setting_arg(&text, "sort").unwrap_or_default().to_string(),
            names: SORT_VALUES.join(", "),
        },
        _ if setting_arg(&text, "net").is_some() => Msg::NetUnknown {
            name: setting_arg(&text, "net").unwrap_or_default().to_string(),
            names: NET_VALUES.join(", "),
        },
        _ => {
            let word = text.split_whitespace().next().unwrap_or_default().to_string();
            if action::by_command(&word).is_some() {
                Msg::CommandNoArgs { name: word }
            } else {
                Msg::CommandUnknown { name: text }
            }
        }
    }
}
