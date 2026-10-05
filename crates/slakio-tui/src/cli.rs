//! Command line: `slakio [--config <path>]`, `--help`, `--version`.

use std::ffi::OsString;
use std::path::PathBuf;

pub(crate) const HELP: &str = "slakio - an unofficial terminal client for Slack (early development:
it does not connect to Slack yet; not affiliated with or endorsed by Slack)

usage: slakio [--config <path>]

  --config <path>    config file (default: $XDG_CONFIG_HOME/slakio/config.toml,
                     else ~/.config/slakio/config.toml)
  -h, --help         show this help
  -V, --version      show the version

environment:
  SLAKIO_SECRET_STORE=memory   keep credentials in memory only, never touch the OS
                               keychain (default: keychain)
  NO_COLOR=1                   draw without colors
";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Cli {
    Run { config: Option<PathBuf> },
    Help,
    Version,
}

/// Parse the arguments (without the program name).
pub(crate) fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Cli, String> {
    use lexopt::prelude::*;
    let mut parser = lexopt::Parser::from_args(args);
    let mut config = None;
    while let Some(arg) = parser.next().map_err(|e| format!("{e}\nusage: slakio [--config <path>]"))? {
        match arg {
            Short('h') | Long("help") => return Ok(Cli::Help),
            Short('V') | Long("version") => return Ok(Cli::Version),
            Long("config") => config = Some(PathBuf::from(parser.value().map_err(|e| e.to_string())?)),
            other => return Err(format!("{}\nusage: slakio [--config <path>]", other.unexpected())),
        }
    }
    Ok(Cli::Run { config })
}

#[cfg(test)]
mod tests;
