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
fn the_language_is_read() {
    assert_eq!(parse("language = \"ko\"\n"), Ok("ko".to_string()));
    assert_eq!(parse("# comment only\n"), Ok("auto".to_string()));
    assert_eq!(parse("language = \"EN\""), Ok("en".to_string()));
}

#[test]
fn a_file_that_cannot_be_used_is_reported_never_taken_as_missing() {
    let Err(ConfigError::Syntax(f)) = parse("language = \n") else { panic!("syntax") };
    assert_eq!(f.kind, FaultKind::Toml { line: Some(1) });
    assert_eq!(parse("theme = \"dark\""), Err(ConfigError::UnknownKey("theme".into())));
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
