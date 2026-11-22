use super::table::{self, SETTINGS, find};
use super::*;
use crate::theme::{Background, Look};
use slakio_core::i18n::Lang;

const TRUECOLOR: Look = Look { truecolor: true, background: Background::Dark, no_color: false };

fn app() -> App {
    let mut a = App::new(Lang::En, Theme::terminal());
    a.load_at_start(TRUECOLOR, &Config::default(), true, Instant::now());
    a.prefs.download_env = None;
    a.open_settings();
    a
}

use crate::theme::Theme;

fn key(a: &mut App, k: &str) {
    for k in crate::keymap::parse_keys(k).unwrap() {
        a.settings_key(k, Instant::now());
    }
}

fn saved(a: &mut App) -> Vec<(&'static str, String)> {
    a.take_effects()
        .into_iter()
        .filter_map(|e| match e {
            Effect::Save { key, value } => Some((key, value)),
            _ => None,
        })
        .collect()
}

fn select(a: &mut App, setting: &str) {
    a.prefs.screen.as_mut().unwrap().selected = find(setting).unwrap();
}

#[test]
fn j_and_k_go_round_the_rows() {
    let mut a = app();
    key(&mut a, "k");
    assert_eq!(a.prefs.screen.as_ref().unwrap().selected, SETTINGS.len() - 1, "up from the first: the last");
    key(&mut a, "j");
    assert_eq!(a.prefs.screen.as_ref().unwrap().selected, 0, "down from the last: the first");
    key(&mut a, "down down up");
    assert_eq!(a.prefs.screen.as_ref().unwrap().selected, 1);
}

#[test]
fn h_and_l_go_round_the_values_and_save_each() {
    let mut a = app();
    select(&mut a, "avatars");
    key(&mut a, "l");
    assert_eq!(a.setting_value(find("avatars").unwrap()), "image");
    key(&mut a, "l l");
    assert_eq!(a.setting_value(find("avatars").unwrap()), "initials", "round again");
    key(&mut a, "h");
    assert_eq!(a.setting_value(find("avatars").unwrap()), "off");
    assert!(!a.settings.avatars);
    assert_eq!(
        saved(&mut a),
        [("avatars", "image"), ("avatars", "off"), ("avatars", "initials"), ("avatars", "off")]
            .map(|(k, v)| (k, v.to_string()))
    );
    select(&mut a, "density");
    key(&mut a, "enter");
    assert!(a.settings.compact, "Enter is the next value too");
    key(&mut a, "space");
    assert!(!a.settings.compact);
    select(&mut a, "nav_rows");
    key(&mut a, "l");
    assert!(a.shell.nav_folded);
    assert_eq!(saved(&mut a).last(), Some(&("nav_rows", "collapsed".to_string())));
}

#[test]
fn the_screen_and_set_change_the_same_state() {
    // Every row is reachable from `:set`, and every key `:set` takes has a row.
    for (k, spec) in SETTINGS.iter().enumerate() {
        let value = match spec.values {
            table::Values::Themes => "dark".to_string(),
            table::Values::Fixed(v) => v[v.len() - 1].0.to_string(),
            table::Values::Folder => "~/x".to_string(),
            table::Values::Megabytes(p) => p[0].to_string(),
        };
        let line = format!("set {}={value}", spec.key);
        assert_eq!(table::parse_set(&line), Some(Ok((k, value.clone()))), "{line}");
        let mut a = app();
        a.apply_setting(k, &value, Instant::now()).unwrap();
        assert_eq!(a.setting_value(k), value, "{}", spec.key);
        assert_eq!(saved(&mut a), [(spec.key, value)]);
    }
    assert!(matches!(table::parse_set("set nope=1"), Some(Err(Msg::SetUnknownKey { .. }))));
}

#[test]
fn a_setting_for_the_next_launch_is_said_so_and_kept_in_prefs() {
    let mut a = app();
    select(&mut a, "language");
    key(&mut a, "l l");
    assert_eq!(a.prefs.config.language, "ko");
    assert_eq!(a.i18n.label(Label::SettingsTitle).to_string(), "Settings", "the language stays until the next launch");
    assert_eq!(a.setting_shown(find("language").unwrap()), ("ko".to_string(), "applies on next launch".to_string()));
    assert_eq!(saved(&mut a), [("language", "en".to_string()), ("language", "ko".to_string())]);
}

#[test]
fn a_custom_size_from_the_file_is_kept_until_changed() {
    let mut a = App::new(Lang::En, Theme::terminal());
    let cfg = Config { download_max_mb: 300, ..Config::default() };
    a.load_at_start(TRUECOLOR, &cfg, true, Instant::now());
    a.open_settings();
    let k = find("download_max_mb").unwrap();
    assert_eq!(a.setting_shown(k).0, "custom (300)");
    select(&mut a, "density");
    key(&mut a, "l");
    assert_eq!(a.setting_shown(k).0, "custom (300)", "another row changed: kept");
    select(&mut a, "download_max_mb");
    key(&mut a, "l");
    assert_eq!((a.prefs.config.download_max_mb, a.downloads.max), (1024, 1_024_000_000), "the next preset up");
    key(&mut a, "h h");
    assert_eq!(a.prefs.config.download_max_mb, 64);
    key(&mut a, "h");
    assert_eq!(a.prefs.config.download_max_mb, 4096, "round again");
}

#[test]
fn the_downloads_folder_is_typed_and_checked() {
    let mut a = app();
    a.downloads.home = Some(PathBuf::from("/home/me"));
    select(&mut a, "downloads");
    key(&mut a, "enter");
    for c in "saved".chars() {
        a.settings_key(KeyChord::char(c), Instant::now());
    }
    key(&mut a, "enter");
    let s = a.prefs.screen.as_ref().unwrap();
    assert_eq!(
        s.folder,
        Some(("saved".to_string(), Some(Msg::SettingsDownloadsRelative { path: "saved".to_string() }))),
        "a relative folder is refused and stays typed"
    );
    assert!(saved(&mut a).is_empty());
    key(&mut a, "ctrl+u");
    for c in "~/x".chars() {
        a.settings_key(KeyChord::char(c), Instant::now());
    }
    key(&mut a, "enter");
    assert_eq!(a.prefs.screen.as_ref().unwrap().folder, None);
    assert_eq!(a.prefs.config.downloads, "~/x");
    assert_eq!(a.downloads.dir, Some(PathBuf::from("/home/me").join("x")), "used from the next download");
    assert_eq!(saved(&mut a), [("downloads", "~/x".to_string())]);
    key(&mut a, "enter esc");
    assert_eq!(a.prefs.screen.as_ref().unwrap().folder, None, "Esc leaves the folder as it was");
    assert_eq!(a.prefs.config.downloads, "~/x");
}

#[test]
fn the_environment_s_downloads_folder_is_shown_and_not_changed() {
    let mut a = app();
    a.prefs.download_env = Some("/tmp/dl".to_string());
    select(&mut a, "downloads");
    key(&mut a, "enter");
    assert_eq!(a.prefs.screen.as_ref().unwrap().folder, None, "nothing to type");
    let (value, what) = a.setting_shown(find("downloads").unwrap());
    assert_eq!((value.as_str(), what.as_str()), ("/tmp/dl", "Set by SLAKIO_DOWNLOAD_DIR: read-only here"));
    let k = find("downloads").unwrap();
    assert!(a.apply_setting(k, "~/y", Instant::now()).is_err(), ":set cannot change it either");
    assert!(saved(&mut a).is_empty());
}

#[test]
fn a_theme_is_shown_then_kept_or_given_back() {
    let mut a = app();
    let before = a.theme.clone();
    key(&mut a, "l l");
    let shown = a.prefs.screen.as_ref().unwrap().themes[2].clone();
    assert_eq!(a.theme, a.theme_named(&shown).unwrap(), "drawn with the theme shown");
    assert!(saved(&mut a).is_empty(), "showing saves nothing");
    key(&mut a, "esc");
    assert_eq!(a.theme, before, "Esc gives the theme back");
    assert!(a.prefs.screen.is_none());
    a.open_settings();
    key(&mut a, "l enter");
    assert_eq!(a.theme_setting, "terminal");
    assert_eq!(saved(&mut a), [("theme", "terminal".to_string())]);
    key(&mut a, "esc");
    assert_eq!(a.theme_setting, "terminal", "kept");
}

/// Typed as the terminal delivers it: one key event per character.
fn typed(a: &mut App, text: &str) {
    use ratatui::crossterm::event::{Event, KeyEvent};
    for c in text.chars() {
        a.handle_event(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)), Instant::now());
    }
}

#[test]
fn hangul_typed_into_the_inputs_stays_hangul() {
    // "~/da-un-ro-deu" and "nae-te-ma" as precomposed syllables, "eo" (the `j` key) as a jamo.
    let mut a = app();
    a.downloads.home = Some(PathBuf::from("/home/me"));
    select(&mut a, "downloads");
    key(&mut a, "enter");
    typed(&mut a, "~/\u{B2E4}\u{C6B4}\u{B85C}\u{B4DC}");
    key(&mut a, "enter");
    assert_eq!(
        saved(&mut a),
        [("downloads", "~/\u{B2E4}\u{C6B4}\u{B85C}\u{B4DC}".to_string())],
        "saved as typed, not as QWERTY keys"
    );
    assert_eq!(a.prefs.config.downloads, "~/\u{B2E4}\u{C6B4}\u{B85C}\u{B4DC}");
    select(&mut a, "theme");
    typed(&mut a, "c");
    key(&mut a, "ctrl+u");
    typed(&mut a, "\u{B0B4}\u{D14C}\u{B9C8}");
    assert_eq!(a.prefs.screen.as_ref().unwrap().name.as_ref().map(|n| n.0.as_str()), Some("\u{B0B4}\u{D14C}\u{B9C8}"));
    key(&mut a, "esc");
    typed(&mut a, "\u{3153}");
    assert_eq!(a.prefs.screen.as_ref().unwrap().selected, find("theme").unwrap() + 1, "the list still moves on jamo");
}

#[test]
fn every_fixed_row_takes_the_config_file_s_values_and_the_palette_suggests_them() {
    use crate::app::palette::{AVATAR_VALUES, DENSITY_VALUES, ICON_VALUES, IMAGE_VALUES};
    let names = |key: &str| match SETTINGS[find(key).unwrap()].values {
        table::Values::Fixed(v) => v.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        _ => unreachable!("{key} is not a fixed row"),
    };
    let sorted = |v: &[&'static str]| {
        let mut v = v.to_vec();
        v.sort_unstable();
        v
    };
    for (key, core) in [
        ("density", config::DENSITIES),
        ("nav_rows", config::NAV_ROWS),
        ("avatars", config::AVATARS),
        ("images", config::IMAGES),
        ("language", config::LANGUAGES),
        ("network", config::NETWORK),
        ("live", config::LIVE),
        ("restore", config::RESTORE),
        ("cache", config::CACHE),
    ] {
        assert_eq!(sorted(&names(key)), sorted(core), "{key}");
    }
    // `ask` is the first launch's question; once answered it is `on` or `off`, all a row shows.
    let icons: Vec<&str> = config::ICONS.iter().copied().filter(|v| *v != "ask").collect();
    assert_eq!(sorted(&names("icons")), sorted(&icons));
    for (key, palette) in
        [("density", DENSITY_VALUES), ("icons", ICON_VALUES), ("avatars", AVATAR_VALUES), ("images", IMAGE_VALUES)]
    {
        assert_eq!(names(key), palette, "{key}: the palette suggests the row's values");
    }
}

#[test]
fn a_paste_goes_into_the_input_open_and_nowhere_else() {
    use ratatui::crossterm::event::Event;
    let mut a = app();
    a.handle_event(Event::Paste("/tmp/x".to_string()), Instant::now());
    assert!(!a.cmdline.is_open() && a.cmdline.text().is_empty(), "no input open: the paste goes nowhere");
    select(&mut a, "downloads");
    key(&mut a, "enter ctrl+u");
    a.handle_event(Event::Paste("/tmp/x\n".to_string()), Instant::now());
    assert_eq!(a.prefs.screen.as_ref().unwrap().folder.as_ref().map(|f| f.0.as_str()), Some("/tmp/x"));
    key(&mut a, "enter");
    assert_eq!(saved(&mut a), [("downloads", "/tmp/x".to_string())]);
}
