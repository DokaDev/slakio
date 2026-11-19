use super::*;

fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
}

#[test]
fn platform_defaults() {
    let home = [("HOME", "/h")];
    let p = Paths::resolve(env(&home), Os::Unix);
    assert_eq!(p.config, Some(Path::new("/h").join(".config").join("slakio")));
    assert_eq!(p.config_file(), Some(Path::new("/h").join(".config").join("slakio").join("config.toml")));
    assert_eq!(p.data, Some(Path::new("/h").join(".local").join("share").join("slakio")));
    assert_eq!(p.state, Some(Path::new("/h").join(".local").join("state").join("slakio")));
    assert_eq!(p.cache, Some(Path::new("/h").join(".cache").join("slakio")));
    let p = Paths::resolve(env(&home), Os::MacOs);
    let support = Path::new("/h").join("Library").join("Application Support").join("slakio");
    assert_eq!(p.config, Some(Path::new("/h").join(".config").join("slakio")), "the same as on Linux");
    assert_eq!(p.data.as_deref(), Some(support.as_path()));
    assert_eq!(p.state, Some(support.join("state")));
    assert_eq!(p.cache, Some(Path::new("/h").join("Library").join("Caches").join("slakio")));
    let win = [("APPDATA", "C:/r"), ("LOCALAPPDATA", "C:/l"), ("USERPROFILE", "C:/u")];
    let p = Paths::resolve(env(&win), Os::Windows);
    assert_eq!(p.config, Some(Path::new("C:/u").join(".config").join("slakio")));
    assert_eq!(p.data, Some(Path::new("C:/r").join("slakio")), "drafts roam");
    assert_eq!(p.state, Some(Path::new("C:/l").join("slakio")), "state stays on the machine");
    assert_eq!(p.cache, Some(Path::new("C:/l").join("slakio").join("cache")));
    assert_eq!(Paths::resolve(env(&[]), Os::Unix), Paths::default(), "no home: nowhere to write");
}

#[test]
fn overrides_and_xdg_win_on_every_os() {
    let vars = [
        ("HOME", "/h"),
        ("XDG_CONFIG_HOME", "/xc"),
        ("XDG_DATA_HOME", "/xd"),
        ("XDG_STATE_HOME", "/xs"),
        ("XDG_CACHE_HOME", "/xk"),
    ];
    for os in [Os::Unix, Os::MacOs, Os::Windows] {
        let p = Paths::resolve(env(&vars), os);
        assert_eq!(p.config, Some(Path::new("/xc").join("slakio")), "{os:?}");
        assert_eq!(p.data, Some(Path::new("/xd").join("slakio")), "{os:?}");
        assert_eq!(p.state, Some(Path::new("/xs").join("slakio")), "{os:?}");
        assert_eq!(p.cache, Some(Path::new("/xk").join("slakio")), "{os:?}");
    }
    let vars = [
        ("XDG_DATA_HOME", "/xd"),
        ("SLAKIO_CONFIG_DIR", "/c"),
        ("SLAKIO_DATA_DIR", "/d"),
        ("SLAKIO_STATE_DIR", "/s"),
        ("SLAKIO_CACHE_DIR", "/k"),
    ];
    let p = Paths::resolve(env(&vars), Os::MacOs);
    assert_eq!(p.config, Some(PathBuf::from("/c")));
    assert_eq!((p.data.clone(), p.state.clone()), (Some(PathBuf::from("/d")), Some(PathBuf::from("/s"))));
    assert_eq!(p.cache, Some(PathBuf::from("/k")));
    assert_eq!(p.notices_file(), Some(Path::new("/s").join("notices.toml")));
    assert_eq!(p.errors_log(), Some(Path::new("/s").join("errors.log")));
    // Empty values count as unset.
    let p = Paths::resolve(env(&[("XDG_DATA_HOME", ""), ("HOME", "/h")]), Os::Unix);
    assert_eq!(p.data, Some(Path::new("/h").join(".local").join("share").join("slakio")));
}

#[test]
fn one_time_notices_are_recorded_in_the_state_directory() {
    let dir = std::env::temp_dir().join(format!("slakio-notices-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let p = Paths { state: Some(dir.join("state")), ..Paths::default() };
    assert!(p.first_time("keychain_unavailable"));
    assert!(!p.first_time("keychain_unavailable"), "shown once");
    assert!(p.first_time("other"));
    assert!(Paths::default().first_time("x") && Paths::default().first_time("x"), "no state dir: always");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_notices_file_that_cannot_be_read_is_never_written_over() {
    let dir = std::env::temp_dir().join(format!("slakio-notices-damaged-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = Paths { state: Some(dir.clone()), ..Paths::default() };
    assert!(p.first_time("a"));
    assert!(!p.first_time("a"), "recorded");
    // Damaged: the notice shows, the file stays as it is.
    std::fs::write(dir.join("notices.toml"), "a = true\nb = [\n").unwrap();
    assert!(p.first_time("c"));
    assert_eq!(std::fs::read_to_string(dir.join("notices.toml")).unwrap(), "a = true\nb = [\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn downloads_go_to_the_override_else_the_home_downloads_folder() {
    let home = PathBuf::from("/home/u");
    let at_home = env(&[("HOME", "/home/u")]);
    assert_eq!(downloads("", &at_home), Some(home.join("Downloads").join("slakio")));
    assert_eq!(downloads("~/Saved/files", &at_home), Some(home.join("Saved/files")));
    // An absolute folder of this system's own kind.
    let srv = if cfg!(windows) { "D:\\srv\\dl" } else { "/srv/dl" };
    assert_eq!(downloads(srv, &at_home), Some(srv.into()));
    assert_eq!(downloads("relative/dl", &at_home), None, "a relative folder depends on where slakio started");
    let scratch = if cfg!(windows) { "D:\\scratch\\dl" } else { "/scratch/dl" };
    let pairs = [("HOME", "/home/u"), ("SLAKIO_DOWNLOAD_DIR", scratch)];
    assert_eq!(downloads(srv, env(&pairs)), Some(scratch.into()));
    // A relative override is no folder, as a relative setting is (not the setting instead).
    let relative = env(&[("HOME", "/home/u"), ("SLAKIO_DOWNLOAD_DIR", "rel/dl")]);
    assert_eq!(downloads(srv, relative), None, "a relative override depends on where slakio started");
    let windows = env(&[("USERPROFILE", "D:\\Profiles\\u")]);
    assert_eq!(downloads("", windows), Some(PathBuf::from("D:\\Profiles\\u").join("Downloads").join("slakio")));
    assert_eq!(downloads("", env(&[("SLAKIO_DOWNLOAD_DIR", "")])), None, "an empty override is no override");
}

#[test]
fn the_demo_saves_in_a_temporary_folder_never_the_users_downloads() {
    let tmp = PathBuf::from(if cfg!(windows) { "D:\\tmp" } else { "/tmp" });
    let home = env(&[("HOME", "/home/u")]);
    assert_eq!(demo_downloads(&home, &tmp), Some(tmp.join("slakio-demo")));
    let scratch = if cfg!(windows) { "D:\\scratch\\dl" } else { "/scratch/dl" };
    let pairs = [("HOME", "/home/u"), ("SLAKIO_DOWNLOAD_DIR", scratch)];
    assert_eq!(demo_downloads(env(&pairs), &tmp), Some(scratch.into()));
    assert_eq!(demo_downloads(env(&[("SLAKIO_DOWNLOAD_DIR", "rel")]), &tmp), None);
}
