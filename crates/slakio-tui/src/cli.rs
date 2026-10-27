//! Command line: `slakio [--demo] [--offline] [--config <path>]`, `slakio login`, `slakio
//! logout`, `--help`, `--version`.

use std::ffi::OsString;
use std::path::PathBuf;

pub(crate) const HELP: &str = "slakio - an unofficial terminal client for Slack (early development:
read-only, one workspace, no live updates yet; not affiliated with or endorsed by Slack)

usage: slakio [--demo] [--offline] [--config <path>]
       slakio login     sign in: paste a request copied from Slack in a browser
       slakio logout    sign out: remove the credentials from the keychain

  --demo             try the interface with an invented workspace (nothing
                     connects anywhere)
  --offline          send nothing to Slack (as SLAKIO_NETWORK=off)
  --config <path>    config file (default: $XDG_CONFIG_HOME/slakio/config.toml,
                     else ~/.config/slakio/config.toml)
  -h, --help         show this help
  -V, --version      show the version

signing in: open your workspace in a browser, open the developer tools
(Network tab), reload, right-click any request to /api/ and choose
Copy > Copy as cURL; then run `slakio login` and paste it (or
`pbpaste | slakio login`). The token and cookies go to the macOS Keychain.

environment:
  SLAKIO_SECRET_STORE=memory   keep credentials in memory only, never touch the OS
                               keychain (default: keychain)
  SLAKIO_NETWORK=off           never send anything to Slack (default: on, unless
                               the config file says network = \"off\")
  NO_COLOR=1                   draw without colors
";

const USAGE: &str = "usage: slakio [--demo] [--offline] [--config <path>] | slakio login | slakio logout";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Cli {
    Run {
        config: Option<PathBuf>,
        demo: bool,
        offline: bool,
    },
    /// Sign in (the app's sign-in screen, or a paste piped in).
    Login {
        config: Option<PathBuf>,
        offline: bool,
    },
    Logout,
    Help,
    Version,
}

/// Parse the arguments (without the program name).
pub(crate) fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Cli, String> {
    use lexopt::prelude::*;
    let mut parser = lexopt::Parser::from_args(args);
    let mut config = None;
    let (mut demo, mut offline) = (false, false);
    let mut command: Option<String> = None;
    while let Some(arg) = parser.next().map_err(|e| format!("{e}\n{USAGE}"))? {
        match arg {
            Short('h') | Long("help") => return Ok(Cli::Help),
            Short('V') | Long("version") => return Ok(Cli::Version),
            Long("demo") => demo = true,
            Long("offline") => offline = true,
            Long("config") => config = Some(PathBuf::from(parser.value().map_err(|e| e.to_string())?)),
            Value(v) if command.is_none() && (v == "login" || v == "logout") => {
                command = v.into_string().ok();
            }
            other => return Err(format!("{}\n{USAGE}", other.unexpected())),
        }
    }
    match command.as_deref() {
        None => Ok(Cli::Run { config, demo, offline }),
        Some(_) if demo => Err(format!("--demo does not sign in or out\n{USAGE}")),
        Some("login") => Ok(Cli::Login { config, offline }),
        _ if offline || config.is_some() => Err(format!("logout takes no options\n{USAGE}")),
        _ => Ok(Cli::Logout),
    }
}

#[cfg(test)]
mod tests;
