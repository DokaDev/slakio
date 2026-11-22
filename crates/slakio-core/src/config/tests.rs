use super::*;
use crate::fault::FaultKind;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-config-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_missing_file_or_no_config_directory_is_the_defaults() {
    assert_eq!(load(None), (Config::default(), None));
    let dir = scratch("missing");
    let path = dir.join("config.toml");
    let (cfg, err) = load(Some(&path));
    assert_eq!((cfg.language.as_str(), cfg.path.as_deref(), err), ("auto", Some(path.as_path()), None));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_settings_are_read() {
    let lang = |t: &str| parse(t).map(|c| c.language);
    assert_eq!(lang("language = \"ko\"\n"), Ok("ko".to_string()));
    assert_eq!(lang("# comment only\n"), Ok("auto".to_string()));
    assert_eq!(lang("language = \"EN\""), Ok("en".to_string()));
    let cfg = parse("icons = true\nrail_expand = \"push\"\ntheme = \"Tokyo-Night\"\n").unwrap();
    assert_eq!((cfg.icons.as_str(), cfg.theme.as_str()), ("on", "tokyo-night"));
    assert_eq!(cfg.retired, ["rail_expand"], "a key of the old rail: ignored, the file still used");
    assert!(parse("unknown_key = 1").is_err(), "any other unknown key is still an error");
    assert_eq!(parse("icons = false").unwrap().icons, "off");
    assert_eq!(parse("avatars = \"off\"").unwrap().avatars, "off");
    assert_eq!(parse("avatars = \"Image\"").unwrap().avatars, "image", "kept for photos, later");
    assert_eq!(Config::default().avatars, "initials");
    assert_eq!(
        parse("avatars = \"photo\""),
        Err(ConfigError::Value {
            key: "avatars".into(),
            value: "\"photo\"".into(),
            allowed: "initials, off, image".into()
        })
    );
    assert_eq!(parse("icons = \"ask\"").unwrap().icons, "ask");
    assert_eq!(Config::default().density, "comfortable");
    assert_eq!(parse("density = \"Compact\"").unwrap().density, "compact");
    assert!(parse("density = \"cozy\"").is_err());
    assert_eq!(Config::default().nav_rows, "expanded");
    assert_eq!(parse("nav_rows = \"Collapsed\"").unwrap().nav_rows, "collapsed");
    assert!(parse("nav_rows = \"folded\"").is_err());
    assert_eq!(Config::default().network, "on");
    assert_eq!(parse("network = \"OFF\"").unwrap().network, "off");
    assert!(parse("network = false").is_err(), "only on or off");
    assert_eq!(Config::default().live, "on");
    assert_eq!(parse("live = \"Off\"").unwrap().live, "off");
    assert!(parse("live = \"sometimes\"").is_err(), "only on or off");
    assert_eq!(parse("").unwrap(), Config::default());
    let d = Config::default();
    assert_eq!((d.icons.as_str(), d.theme.as_str()), ("ask", "auto"));
    assert!(d.retired.is_empty());
}

#[test]
fn a_bad_value_names_the_allowed_ones() {
    assert_eq!(parse("rail_expand = \"side\"").map(|c| c.retired), Ok(vec!["rail_expand".to_string()]), "never judged");
    assert_eq!(
        parse("icons = \"yes\""),
        Err(ConfigError::Value { key: "icons".into(), value: "\"yes\"".into(), allowed: "on, off, ask".into() })
    );
}

#[test]
fn the_theme_is_a_built_in_or_a_theme_file_name() {
    let theme = |t: &str| parse(t).map(|c| c.theme);
    assert_eq!(theme("theme = \"mine\""), Ok("mine".to_string()), "a theme file's name");
    assert_eq!(theme("theme = \"My_Theme-2\""), Ok("My_Theme-2".to_string()), "kept as written");
    assert_eq!(theme("theme = \"Dark\""), Ok("dark".to_string()), "a built-in, ignoring case");
    let allowed = format!("{}, or a theme file's name", THEMES.join(", "));
    for bad in ["../x", "a.b", "", "x/y"] {
        let text = format!("theme = \"{bad}\"");
        assert_eq!(
            parse(&text),
            Err(ConfigError::Value { key: "theme".into(), value: format!("\"{bad}\""), allowed: allowed.clone() }),
            "{text}"
        );
    }
    assert!(parse("theme = 3").is_err());
    assert_eq!(theme_name("NORD").as_deref(), Some("nord"));
    assert_eq!(theme_name(&"x".repeat(65)), None);
}

#[test]
fn a_file_that_cannot_be_used_is_reported_never_taken_as_missing() {
    let Err(ConfigError::Syntax(f)) = parse("language = \n") else { panic!("syntax") };
    assert_eq!(f.kind, FaultKind::Toml { line: Some(1) });
    assert_eq!(parse("colour = \"dark\""), Err(ConfigError::UnknownKey("colour".into())));
    assert_eq!(
        parse("language = \"fr\""),
        Err(ConfigError::Value { key: "language".into(), value: "\"fr\"".into(), allowed: "auto, en, ko".into() })
    );
    assert!(matches!(parse("language = 3"), Err(ConfigError::Value { .. })));
    // A directory where the file should be: unreadable, not missing.
    let dir = scratch("unreadable");
    let path = dir.join("config.toml");
    std::fs::create_dir_all(&path).unwrap();
    let (cfg, err) = load(Some(&path));
    assert_eq!(cfg.language, "auto");
    assert!(matches!(err, Some(ConfigError::Read { .. })), "{err:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_answer_is_saved_keeping_what_the_file_holds() {
    let dir = scratch("set");
    let path = dir.join("sub").join("config.toml");
    set(&path, "icons", "on").unwrap();
    assert_eq!(load(Some(&path)).0.icons, "on", "a missing file is created");
    std::fs::write(&path, "# mine\nlanguage = \"ko\" # keep\nicons = \"ask\"\n").unwrap();
    set(&path, "icons", "off").unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# mine\nlanguage = \"ko\" # keep\n"), "{text}");
    assert_eq!(load(Some(&path)).0.icons, "off");
    std::fs::write(&path, "language = \n").unwrap();
    assert!(set(&path, "icons", "on").is_err(), "a broken file is never written over");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "language = \n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_saved_value_keeps_its_same_line_comment() {
    let dir = scratch("decor");
    let path = dir.join("config.toml");
    std::fs::write(&path, "# mine\ntheme = \"tokyo-night\"  # owner pick\nicons = \"on\"\n").unwrap();
    set(&path, "theme", "dark").unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text, "# mine\ntheme = \"dark\"  # owner pick\nicons = \"on\"\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_number_is_saved_as_a_number() {
    let dir = scratch("number");
    let path = dir.join("config.toml");
    std::fs::write(&path, "# mine\ndownload_max_mb = 300  # big ones\n").unwrap();
    set(&path, "download_max_mb", "256").unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text, "# mine\ndownload_max_mb = 256  # big ones\n");
    assert_eq!(parse(&text).unwrap().download_max_mb, 256);
    set(&path, "downloads", "123").unwrap();
    assert!(std::fs::read_to_string(&path).unwrap().contains("downloads = \"123\""), "text stays text");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_number_setting_is_never_written_as_text() {
    let dir = scratch("not-a-number");
    let path = dir.join("config.toml");
    let text = "download_max_mb = 300\n";
    std::fs::write(&path, text).unwrap();
    for bad in ["x", "-1", "1.5", ""] {
        assert!(set(&path, "download_max_mb", bad).is_err(), "{bad}");
        assert!(set(&path, "cache_days", bad).is_err(), "{bad}");
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "the file is unchanged");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_cache_settings_are_read_within_their_ranges() {
    let d = Config::default();
    assert_eq!((d.cache.as_str(), d.cache_messages, d.cache_max_mb, d.cache_days), ("on", 200, 500, 30));
    let cfg = parse("cache = \"OFF\"\ncache_messages = 50\ncache_max_mb = 100\ncache_days = 7\n").unwrap();
    assert_eq!((cfg.cache.as_str(), cfg.cache_messages, cfg.cache_max_mb, cfg.cache_days), ("off", 50, 100, 7));
    // The ends of each range are in it.
    assert_eq!(parse("cache_messages = 10").unwrap().cache_messages, 10);
    assert_eq!(parse("cache_days = 3650").unwrap().cache_days, 3650);
    for bad in [
        "cache = true",
        "cache = \"maybe\"",
        "cache_messages = 9",
        "cache_messages = 10001",
        "cache_messages = -1",
        "cache_messages = \"200\"",
        "cache_messages = 2.5",
        "cache_max_mb = 0",
        "cache_max_mb = 100001",
        "cache_days = 0",
        "cache_days = 99999999999",
    ] {
        assert!(matches!(parse(bad), Err(ConfigError::Value { .. })), "{bad}");
    }
    assert_eq!(
        parse("cache_days = 0"),
        Err(ConfigError::Value {
            key: "cache_days".into(),
            value: "0".into(),
            allowed: "a whole number from 1 to 3650".into()
        })
    );
}

#[test]
fn restoring_the_last_layout_is_on_unless_turned_off() {
    assert_eq!(Config::default().restore, "on");
    assert_eq!(parse("restore = \"Off\"").unwrap().restore, "off");
    assert_eq!(parse("restore = \"on\"").unwrap().restore, "on");
    for bad in ["restore = false", "restore = \"later\""] {
        assert!(matches!(parse(bad), Err(ConfigError::Value { .. })), "{bad}");
    }
}

#[test]
fn the_download_settings_are_read_within_their_ranges() {
    let d = Config::default();
    assert_eq!((d.downloads.as_str(), d.download_max_mb), ("", 1024));
    let cfg = parse("downloads = \"~/Saved\"\ndownload_max_mb = 50\n").unwrap();
    assert_eq!((cfg.downloads.as_str(), cfg.download_max_mb), ("~/Saved", 50));
    for bad in ["download_max_mb = 0", "download_max_mb = 100001", "downloads = 3"] {
        assert!(matches!(parse(bad), Err(ConfigError::Value { .. })), "{bad}");
    }
}

#[cfg(unix)]
#[test]
fn a_saved_setting_is_the_user_s_alone() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("private");
    let path = dir.join("slakio").join("config.toml");
    set(&path, "images", "off").unwrap();
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&path), 0o600, "the file");
    assert_eq!(mode(path.parent().unwrap()), 0o700, "the folder made for it");
    // A file there already, readable by others, is the user's alone once written.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    set(&path, "images", "auto").unwrap();
    assert_eq!(mode(&path), 0o600);
    let _ = std::fs::remove_dir_all(&dir);
}
