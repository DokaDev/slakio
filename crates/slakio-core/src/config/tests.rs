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
    assert_eq!(cfg.rail_expand, "push");
    assert_eq!(parse("icons = false").unwrap().icons, "off");
    assert_eq!(parse("icons = \"ask\"").unwrap().icons, "ask");
    assert_eq!(parse("").unwrap(), Config::default());
    let d = Config::default();
    assert_eq!((d.icons.as_str(), d.theme.as_str(), d.rail_expand.as_str()), ("ask", "auto", "overlay"));
}

#[test]
fn a_bad_value_names_the_allowed_ones() {
    assert_eq!(
        parse("rail_expand = \"side\""),
        Err(ConfigError::Value {
            key: "rail_expand".into(),
            value: "\"side\"".into(),
            allowed: "overlay, push".into()
        })
    );
    assert_eq!(
        parse("icons = \"yes\""),
        Err(ConfigError::Value { key: "icons".into(), value: "\"yes\"".into(), allowed: "on, off, ask".into() })
    );
    assert!(matches!(parse("theme = \"solarized\""), Err(ConfigError::Value { .. })));
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
