use super::*;

fn parse(args: &[&str]) -> Result<Cli, String> {
    parse_args(args.iter().map(OsString::from))
}

#[test]
fn options() {
    assert_eq!(parse(&[]), Ok(Cli::Run { config: None, demo: false }));
    assert_eq!(parse(&["--demo"]), Ok(Cli::Run { config: None, demo: true }));
    assert_eq!(parse(&["--version"]), Ok(Cli::Version));
    assert_eq!(parse(&["-V"]), Ok(Cli::Version));
    assert_eq!(parse(&["-h"]), Ok(Cli::Help));
    assert_eq!(
        parse(&["--config", "/tmp/c.toml"]),
        Ok(Cli::Run { config: Some(PathBuf::from("/tmp/c.toml")), demo: false })
    );
    assert_eq!(
        parse(&["--config=/tmp/c.toml"]),
        Ok(Cli::Run { config: Some(PathBuf::from("/tmp/c.toml")), demo: false })
    );
}

#[test]
fn mistakes_are_errors_with_the_usage() {
    for args in [&["--demo=1"][..], &["extra"], &["--config"]] {
        let e = parse(args).unwrap_err();
        assert!(!e.is_empty(), "{args:?}");
    }
    assert!(parse(&["--nope"]).unwrap_err().contains("usage: slakio"));
}
