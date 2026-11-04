//! The settings the palette changes and saves in the config file: `:theme`, `:avatars`,
//! `:density`, `:icons` (each with a value, or alone: the other value).

use super::{DENSITY_VALUES, ICON_VALUES};
use crate::action::AppAction;
use crate::app::{App, Effect};
use crate::theme;
use slakio_core::i18n::Msg;
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

    /// `:avatars <initials|off>`: picture people by their initials chip, or not, from now on,
    /// and save it in the config file. Another value changes nothing and says which ones work.
    pub fn set_avatars(&mut self, value: &str, now: Instant) -> Result<(), Msg> {
        let value = value.trim().to_ascii_lowercase();
        if !AVATAR_VALUES.contains(&value.as_str()) {
            return Err(Msg::AvatarsUnknown { name: value, names: AVATAR_VALUES.join(", ") });
        }
        self.settings.avatars = value == "initials";
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

/// The values `:avatars` takes (the config file also keeps `image` for photos, later).
pub const AVATAR_VALUES: &[&str] = &["initials", "off"];

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
