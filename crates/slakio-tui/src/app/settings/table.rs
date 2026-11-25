//! The settings table: every setting the settings screen shows and `:set <key>=<value>` changes,
//! in screen order (by group), with the values it takes and what it is for. The screen and
//! `:set` read this one table, so a setting is in both or in neither.

use slakio_core::config;
use slakio_core::i18n::{Label, Msg};
use slakio_core::sanitize::Remote;

/// The groups of the settings screen, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Display,
    Pictures,
    Language,
    Files,
    Network,
    Sidebar,
}

impl Group {
    pub fn label(self) -> Label {
        match self {
            Group::Display => Label::SettingsGroupDisplay,
            Group::Pictures => Label::SettingsGroupPictures,
            Group::Language => Label::SettingsGroupLanguage,
            Group::Files => Label::SettingsGroupFiles,
            Group::Network => Label::SettingsGroupNetwork,
            Group::Sidebar => Label::SettingsGroupSidebar,
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
    /// Its name on the settings screen.
    pub label: Label,
    pub group: Group,
    pub values: Values,
    /// What it is for, under the list when it is selected.
    pub about: Label,
    /// It takes effect at the next launch only.
    pub restart: bool,
}

const ON_OFF: &[(&str, Option<Label>)] = &[("on", None), ("off", None)];

const fn row(key: &'static str, label: Label, group: Group, values: Values, about: Label) -> Spec {
    Spec { key, label, group, values, about, restart: false }
}

const fn on_restart(key: &'static str, label: Label, group: Group, values: Values, about: Label) -> Spec {
    Spec { key, label, group, values, about, restart: true }
}

/// Every setting, in the screen's order.
pub const SETTINGS: &[Spec] = &[
    row("theme", Label::SettingsLabelTheme, Group::Display, Values::Themes, Label::SettingsAboutTheme),
    row(
        "density",
        Label::SettingsLabelDensity,
        Group::Display,
        Values::Fixed(&[("comfortable", Some(Label::DensityComfortable)), ("compact", Some(Label::DensityCompact))]),
        Label::SettingsAboutDensity,
    ),
    row(
        "nav_rows",
        Label::SettingsLabelNavRows,
        Group::Display,
        Values::Fixed(&[
            ("expanded", Some(Label::SettingsNavRowsExpanded)),
            ("collapsed", Some(Label::SettingsNavRowsCollapsed)),
        ]),
        Label::SettingsAboutNavRows,
    ),
    row(
        "icons",
        Label::SettingsLabelIcons,
        Group::Display,
        Values::Fixed(&[("on", Some(Label::IconsOn)), ("off", Some(Label::IconsOff))]),
        Label::SettingsAboutIcons,
    ),
    row(
        "avatars",
        Label::SettingsLabelAvatars,
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
        Label::SettingsLabelImages,
        Group::Pictures,
        Values::Fixed(&[
            ("auto", Some(Label::ImagesAuto)),
            ("on", Some(Label::ImagesOn)),
            ("off", Some(Label::ImagesOff)),
        ]),
        Label::SettingsAboutImages,
    ),
    row(
        "animate",
        Label::SettingsLabelAnimate,
        Group::Pictures,
        Values::Fixed(&[("on", Some(Label::AnimateOn)), ("off", Some(Label::AnimateOff))]),
        Label::SettingsAboutAnimate,
    ),
    on_restart(
        "language",
        Label::SettingsLabelLanguage,
        Group::Language,
        Values::Fixed(&[("auto", None), ("en", None), ("ko", None)]),
        Label::SettingsAboutLanguage,
    ),
    row("downloads", Label::SettingsLabelDownloads, Group::Files, Values::Folder, Label::SettingsAboutDownloads),
    row(
        "download_max_mb",
        Label::SettingsLabelDownloadMaxMb,
        Group::Files,
        Values::Megabytes(&[64, 256, 1024, 4096]),
        Label::SettingsAboutDownloadMaxMb,
    ),
    on_restart(
        "network",
        Label::SettingsLabelNetwork,
        Group::Network,
        Values::Fixed(ON_OFF),
        Label::SettingsAboutNetwork,
    ),
    on_restart("live", Label::SettingsLabelLive, Group::Network, Values::Fixed(ON_OFF), Label::SettingsAboutLive),
    on_restart(
        "restore",
        Label::SettingsLabelRestore,
        Group::Network,
        Values::Fixed(ON_OFF),
        Label::SettingsAboutRestore,
    ),
    on_restart("cache", Label::SettingsLabelCache, Group::Network, Values::Fixed(ON_OFF), Label::SettingsAboutCache),
    row(
        "channel_sort",
        Label::SettingsLabelChannelSort,
        Group::Sidebar,
        Values::Fixed(&[
            ("alphabetical", Some(Label::SettingsSortAlphabetical)),
            ("recent", Some(Label::SettingsSortRecent)),
        ]),
        Label::SettingsAboutChannelSort,
    ),
    row(
        "dm_sort",
        Label::SettingsLabelDmSort,
        Group::Sidebar,
        Values::Fixed(&[
            ("recent", Some(Label::SettingsSortRecent)),
            ("alphabetical", Some(Label::SettingsSortAlphabetical)),
        ]),
        Label::SettingsAboutDmSort,
    ),
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
        return Some(Err(Msg::SetUnknownKey { key: shown(key), keys }));
    };
    let Some(value) = value else { return Some(Err(Msg::SetNoValue { key: shown(key) })) };
    Some(check(k, value).map(|v| (k, v)))
}

/// `value` as setting `k` takes it (a fixed value lowercased), or why it is not one.
pub fn check(k: usize, value: &str) -> Result<String, Msg> {
    let spec = &SETTINGS[k];
    let bad = |values: String| Msg::SetBadValue { key: spec.key.to_string(), value: shown(value), values };
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

/// A word typed, safe to print on one line.
fn shown(text: &str) -> String {
    Remote::new(text).line().into_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_groups_are_in_screen_order_and_each_once() {
        let groups: Vec<Group> = groups().into_iter().map(|(g, _)| g).collect();
        assert_eq!(
            groups,
            [Group::Display, Group::Pictures, Group::Language, Group::Files, Group::Network, Group::Sidebar]
        );
    }

    /// A group's heading never reads as one of its rows (Korean once named the pictures group and
    /// the images row alike): in every language no group is named as any row is.
    #[test]
    fn no_group_is_named_as_a_row() {
        use slakio_core::i18n::{I18n, Lang};
        let groups = [Group::Display, Group::Pictures, Group::Language, Group::Files, Group::Network, Group::Sidebar];
        for lang in [Lang::En, Lang::Ko] {
            let i18n = I18n::new(lang);
            for g in groups {
                let heading = i18n.label(g.label()).to_string();
                for s in SETTINGS {
                    let row = i18n.label(s.label).to_string();
                    assert_ne!(heading.trim(), row.trim(), "{lang:?}: {g:?} and the {} row", s.key);
                }
            }
        }
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

    #[test]
    fn typed_words_are_said_safe_to_print() {
        // What was typed is said safe to print: no escape, no bidi control.
        for line in ["set live=\x1b[31m", "set \x1b[31m=on", "set \u{202e}x"] {
            let Some(Err(msg)) = parse_set(line) else { panic!("{line:?} is refused") };
            let said = match msg {
                Msg::SetBadValue { value, .. } => value,
                Msg::SetUnknownKey { key, .. } | Msg::SetNoValue { key } => key,
                other => panic!("{other:?}"),
            };
            assert!(!said.contains('\x1b') && !said.contains('\u{202e}'), "{said}");
        }
    }
}
