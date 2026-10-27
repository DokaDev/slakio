use super::*;

fn parse(args: &[&str]) -> Result<Cli, String> {
    parse_args(args.iter().map(OsString::from))
}

fn run(config: Option<&str>, demo: bool, offline: bool) -> Result<Cli, String> {
    Ok(Cli::Run { config: config.map(PathBuf::from), demo, offline })
}

#[test]
fn options() {
    assert_eq!(parse(&[]), run(None, false, false));
    assert_eq!(parse(&["--demo"]), run(None, true, false));
    assert_eq!(parse(&["--offline"]), run(None, false, true));
    assert_eq!(parse(&["--version"]), Ok(Cli::Version));
    assert_eq!(parse(&["-V"]), Ok(Cli::Version));
    assert_eq!(parse(&["-h"]), Ok(Cli::Help));
    assert_eq!(parse(&["--config", "/tmp/c.toml"]), run(Some("/tmp/c.toml"), false, false));
    assert_eq!(parse(&["--config=/tmp/c.toml"]), run(Some("/tmp/c.toml"), false, false));
}

#[test]
fn login_and_logout() {
    assert_eq!(parse(&["login"]), Ok(Cli::Login { config: None, offline: false, paste: false }));
    assert_eq!(parse(&["--offline", "login"]), Ok(Cli::Login { config: None, offline: true, paste: false }));
    assert_eq!(parse(&["login", "--paste"]), Ok(Cli::Login { config: None, offline: false, paste: true }));
    assert_eq!(parse(&["logout"]), Ok(Cli::Logout));
    for args in [
        &["login", "--demo"][..],
        &["logout", "--offline"],
        &["login", "logout"],
        &["login", "x"],
        &["--paste"],
        &["logout", "--paste"],
    ] {
        assert!(parse(args).unwrap_err().contains("usage: slakio"), "{args:?}");
    }
}

#[test]
fn mistakes_are_errors_with_the_usage() {
    for args in [&["--demo=1"][..], &["extra"], &["--config"]] {
        let e = parse(args).unwrap_err();
        assert!(!e.is_empty(), "{args:?}");
    }
    assert!(parse(&["--nope"]).unwrap_err().contains("usage: slakio"));
}
