//! Opening a link in the user's browser: only a web address (`http`, `https`) whose host reads
//! plainly, handed to the system's opener as one argument (no shell reads it): `open` on macOS,
//! `xdg-open` on other Unix systems, `rundll32 url.dll,FileProtocolHandler` on Windows. Nothing
//! is sent to Slack.

use std::io;
use std::process::{Command, Stdio};

/// `SLAKIO_TEST_OPENER`, debug builds only: the program run instead of the system's opener (the
/// tests record what it is handed; no browser starts).
const TEST_OPENER: &str = "SLAKIO_TEST_OPENER";

/// The host of `url` when it may be opened: `http://` or `https://`, no space or control
/// character, a host of ASCII letters, digits, `-` and dots (no user or password before it);
/// lower case, its port left out. `None` for anything else (`javascript:`, `data:`, `file:`,
/// `slack:`, `mailto:` and every other scheme).
pub fn openable(url: &str) -> Option<String> {
    if url.is_empty() || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.contains(['@', '\\']) {
        return None;
    }
    let host = match authority.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit()) => h,
        Some(_) => return None,
        None => authority,
    };
    let plain = !host.is_empty()
        && host.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    plain.then(|| host.to_string())
}

/// The command that hands `url` to the system's opener (or the tests' stand-in), not started.
pub fn command(url: &str) -> Command {
    if cfg!(debug_assertions)
        && let Some(program) = std::env::var_os(TEST_OPENER)
    {
        let mut c = Command::new(program);
        c.arg(url);
        return c;
    }
    system(url)
}

#[cfg(target_os = "macos")]
fn system(url: &str) -> Command {
    let mut c = Command::new("open");
    c.arg(url);
    c
}

#[cfg(windows)]
fn system(url: &str) -> Command {
    let mut c = Command::new("rundll32");
    c.arg("url.dll,FileProtocolHandler").arg(url);
    c
}

#[cfg(not(any(target_os = "macos", windows)))]
fn system(url: &str) -> Command {
    let mut c = Command::new("xdg-open");
    c.arg(url);
    c
}

/// Open `url` (checked again: [`openable`]) without waiting for the opener, which is reaped on a
/// thread of its own; its input and output go nowhere (never into the terminal).
pub fn open(url: &str) -> io::Result<()> {
    if openable(url).is_none() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not an http or https address"));
    }
    let mut child = command(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plain_web_address_is_opened() {
        assert_eq!(openable("https://example.com/a?b=1#c").as_deref(), Some("example.com"));
        assert_eq!(openable("HTTP://Docs.Example.org:8080/x").as_deref(), Some("docs.example.org"));
        for refused in [
            "javascript:alert(1)",
            "JavaScript://example.com/%0aalert(1)",
            "data:text/html,<b>x</b>",
            "file:///etc/passwd",
            "slack://channel?id=C1",
            "mailto:a@example.com",
            "ftp://example.com/x",
            "https://user:pw@example.com/",
            "https://example.com\\@evil.test/",
            "https://exa mple.com/",
            "https://example.com/\u{1b}[31m",
            "https://bücher.example/",
            "https://example..com/",
            "https://example.com:99999999/",
            "https:///nohost",
            "",
        ] {
            assert_eq!(openable(refused), None, "{refused:?}");
            assert!(open(refused).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn the_opener_is_handed_the_address_as_one_argument_without_a_shell() {
        let url = "https://example.com/a?b=1&c=$(touch x);d";
        let c = system(url);
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args.last().map(String::as_str), Some(url), "{args:?}");
        let program = c.get_program().to_string_lossy().into_owned();
        assert!(["open", "xdg-open", "rundll32"].contains(&program.as_str()), "{program}");
        assert!(!["sh", "bash", "cmd", "powershell"].contains(&program.as_str()));
    }
}
