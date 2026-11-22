//! The settings table: every setting the settings screen shows and `:set <key>=<value>` changes,
//! in screen order (by group), with the values it takes and what it is for. The screen and
//! `:set` read this one table, so a setting is in both or in neither.

use slakio_core::config;
use slakio_core::i18n::{Label, Msg};

/// The groups of the settings screen, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Display,
    Pictures,
    Language,
    Files,
    Network,
}

impl Group {
    pub fn label(self) -> Label {
        match self {
            Group::Display => Label::SettingsGroupDisplay,
            Group::Pictures => Label::SettingsGroupPictures,
            Group::Language => Label::SettingsGroupLanguage,
            Group::Files => Label::SettingsGroupFiles,
            Group::Network => Label::SettingsGroupNetwork,
        }
    }
}

/// The values a setting takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Values {
    /// The built-in themes and the theme files there are.
    Themes,
    /// One of these, each with what it means (none: the row says when it applies instead).
    Fixed(&'static [(&'static str, Option<Label>)]),
    /// A folder, typed in.
    Folder,
    /// Megabytes: one of these presets, or any whole number `:set` or the file gives.
    Megabytes(&'static [u32]),
}

/// One setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    /// Its key in the config file and after `:set`.
    pub key: &'static str,
    pub group: Group,
    pub values: Values,
    /// What it is for, under the list when it is selected.
    pub about: Label,
    /// It takes effect at the next launch only.
    pub restart: bool,
}

const ON_OFF: &[(&str, Option<Label>)] = &[("on", None), ("off", None)];

const fn row(key: &'static str, group: Group, values: Values, about: Label) -> Spec {
    Spec { key, group, values, about, restart: false }
}

const fn on_restart(key: &'static str, group: Group, values: Values, about: Label) -> Spec {
    Spec { key, group, values, about, restart: true }
}

/// Every setting, in the screen's order.
pub const SETTINGS: &[Spec] = &[
    row("theme", Group::Display, Values::Themes, Label::SettingsAboutTheme),
    row(
        "density",
        Group::Display,
        Values::Fixed(&[("comfortable", Some(Label::DensityComfortable)), ("compact", Some(Label::DensityCompact))]),
        Label::SettingsAboutDensity,
    ),
    row(
        "nav_rows",
        Group::Display,
        Values::Fixed(&[
            ("expanded", Some(Label::SettingsNavRowsExpanded)),
            ("collapsed", Some(Label::SettingsNavRowsCollapsed)),
        ]),
        Label::SettingsAboutNavRows,
    ),
    row(
        "icons",
        Group::Display,
        Values::Fixed(&[("on", Some(Label::IconsOn)), ("off", Some(Label::IconsOff))]),
        Label::SettingsAboutIcons,
    ),
    row(
        "avatars",
        Group::Pictures,
        Values::Fixed(&[
            ("initials", Some(Label::AvatarsInitials)),
            ("image", Some(Label::AvatarsImage)),
            ("off", Some(Label::AvatarsOff)),
        ]),
        Label::SettingsAboutAvatars,
    ),
    row(
        "images",
        Group::Pictures,
        Values::Fixed(&[
            ("auto", Some(Label::ImagesAuto)),
            ("on", Some(Label::ImagesOn)),
            ("off", Some(Label::ImagesOff)),
        ]),
        Label::SettingsAboutImages,
    ),
    on_restart(
        "language",
        Group::Language,
        Values::Fixed(&[("auto", None), ("en", None), ("ko", None)]),
        Label::SettingsAboutLanguage,
    ),
    row("downloads", Group::Files, Values::Folder, Label::SettingsAboutDownloads),
    row("download_max_mb", Group::Files, Values::Megabytes(&[64, 256, 1024, 4096]), Label::SettingsAboutDownloadMaxMb),
    on_restart("network", Group::Network, Values::Fixed(ON_OFF), Label::SettingsAboutNetwork),
    on_restart("live", Group::Network, Values::Fixed(ON_OFF), Label::SettingsAboutLive),
    on_restart("restore", Group::Network, Values::Fixed(ON_OFF), Label::SettingsAboutRestore),
    on_restart("cache", Group::Network, Values::Fixed(ON_OFF), Label::SettingsAboutCache),
];

/// The row of setting `key`.
pub fn find(key: &str) -> Option<usize> {
    SETTINGS.iter().position(|s| s.key == key)
}

/// The groups in screen order, each with its rows (indices into [`SETTINGS`]).
pub fn groups() -> Vec<(Group, Vec<usize>)> {
    let mut out: Vec<(Group, Vec<usize>)> = Vec::new();
    for (i, s) in SETTINGS.iter().enumerate() {
        match out.last_mut() {
            Some((g, rows)) if *g == s.group => rows.push(i),
            _ => out.push((s.group, vec![i])),
        }
    }
    out
}

/// `:set <key>=<value>`: the row and the value it is set to (checked as far as the table can:
/// a theme or a folder is checked when it is used), or why not. `None`: the line is not `:set`.
pub fn parse_set(line: &str) -> Option<Result<(usize, String), Msg>> {
    let rest = line.trim().strip_prefix("set")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim();
    let (key, value) = match rest.split_once('=') {
        Some((k, v)) => (k.trim(), Some(v.trim())),
        None => (rest, None),
    };
    let Some(k) = find(key) else {
        let keys = SETTINGS.iter().map(|s| s.key).collect::<Vec<_>>().join(", ");
        return Some(Err(Msg::SetUnknownKey { key: key.to_string(), keys }));
    };
    let Some(value) = value else { return Some(Err(Msg::SetNoValue { key: key.to_string() })) };
    Some(check(k, value).map(|v| (k, v)))
}

/// `value` as setting `k` takes it (a fixed value lowercased), or why it is not one.
pub fn check(k: usize, value: &str) -> Result<String, Msg> {
    let spec = &SETTINGS[k];
    let bad = |values: String| Msg::SetBadValue { key: spec.key.to_string(), value: value.to_string(), values };
    match spec.values {
        Values::Themes | Values::Folder => Ok(value.to_string()),
        Values::Fixed(values) => {
            let lower = value.to_ascii_lowercase();
            match values.iter().find(|(name, _)| *name == lower) {
                Some((name, _)) => Ok((*name).to_string()),
                None => Err(bad(values.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "))),
            }
        }
        Values::Megabytes(_) => {
            let range = config::DOWNLOAD_MAX_MB;
            match value.parse::<u32>() {
                Ok(n) if range.contains(&n) => Ok(n.to_string()),
                _ => Err(bad(format!("{}-{}", range.start(), range.end()))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_groups_are_in_screen_order_and_each_once() {
        let groups: Vec<Group> = groups().into_iter().map(|(g, _)| g).collect();
        assert_eq!(groups, [Group::Display, Group::Pictures, Group::Language, Group::Files, Group::Network]);
    }

    #[test]
    fn every_key_is_a_config_key() {
        let file = SETTINGS.iter().map(|s| format!("{} = {}\n", s.key, sample(s))).collect::<String>();
        let dir = std::env::temp_dir().join(format!("slakio-settings-keys-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, file).unwrap();
        let (_, err) = config::load(Some(&path));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(err, None, "every row is a key the config file takes");
    }

    fn sample(s: &Spec) -> String {
        match s.values {
            Values::Themes => "\"dark\"".to_string(),
            Values::Fixed(v) => format!("\"{}\"", v[0].0),
            Values::Folder => "\"~/x\"".to_string(),
            Values::Megabytes(p) => p[0].to_string(),
        }
    }

    #[test]
    fn set_takes_the_table_s_keys_and_values() {
        assert_eq!(parse_set("set density=Compact"), Some(Ok((find("density").unwrap(), "compact".to_string()))));
        assert_eq!(parse_set("  set  restore = off "), Some(Ok((find("restore").unwrap(), "off".to_string()))));
        assert_eq!(parse_set("set download_max_mb=300"), Some(Ok((find("download_max_mb").unwrap(), "300".into()))));
        assert_eq!(parse_set("settings"), None);
        assert_eq!(parse_set("density compact"), None);
        assert!(matches!(parse_set("set colour=red"), Some(Err(Msg::SetUnknownKey { .. }))));
        assert!(matches!(parse_set("set live=maybe"), Some(Err(Msg::SetBadValue { .. }))));
        assert!(matches!(parse_set("set download_max_mb=0"), Some(Err(Msg::SetBadValue { .. }))));
        assert!(matches!(parse_set("set live"), Some(Err(Msg::SetNoValue { .. }))));
    }
}
