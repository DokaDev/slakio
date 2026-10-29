//! Opening a link in the user's browser: only a web address (`http`, `https`) whose host reads
//! plainly, handed to the system's opener as one argument (no shell reads it): `/usr/bin/open`
//! on macOS, `xdg-open` found on `PATH` on other Unix systems (as desktops expect), and
//! `%SystemRoot%\System32\rundll32.exe url.dll,FileProtocolHandler` on Windows. Nothing is sent to
//! Slack.

use std::io;
use std::process::{Command, Stdio};

/// `SLAKIO_TEST_OPENER`, debug builds only: the program run instead of the system's opener (the
/// tests record what it is handed; no browser starts).
const TEST_OPENER: &str = "SLAKIO_TEST_OPENER";

/// The longest address opened.
pub const URL_MAX: usize = 8192;

/// An address that may be opened, as handed to the opener, and the host it goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checked {
    /// The address, its scheme and host in lower case and every byte past the host that is not
    /// plain ASCII (or one a parser may read otherwise) percent-encoded.
    pub url: String,
    /// The host, lower case, its port left out.
    pub host: String,
}

/// `url` when it may be opened: `http://` or `https://`, at most [`URL_MAX`] bytes, no space or
/// control character, a host of ASCII letters, digits, `-` and dots within the DNS limits (253
/// bytes, labels of 63) and no user or password before it; a host that reads as a number (an
/// IPv4 address in any form but four plain decimal parts) is refused, as browsers read it as
/// one. `None` for anything else (`javascript:`, `data:`, `file:`, `slack:`, `mailto:` and every
/// other scheme).
pub fn check(url: &str) -> Option<Checked> {
    if url.is_empty() || url.len() > URL_MAX || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let scheme_len = if url.get(..8)?.eq_ignore_ascii_case("https://") {
        8
    } else if url.get(..7)?.eq_ignore_ascii_case("http://") {
        7
    } else {
        return None;
    };
    let rest = &url[scheme_len..];
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    if authority.contains(['@', '\\', '%']) || !authority.is_ascii() {
        return None;
    }
    let authority = authority.to_ascii_lowercase();
    let host = match authority.rsplit_once(':') {
        Some((h, port)) if !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit()) => h,
        Some(_) => return None,
        None => authority.as_str(),
    };
    let labels: Vec<&str> = host.split('.').collect();
    let plain = !host.is_empty()
        && host.len() <= 253
        && labels
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 63 && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    if !plain || (numeric(labels.last()?) && !dotted_quad(&labels)) {
        return None;
    }
    let mut out = format!("{}{authority}", url[..scheme_len].to_ascii_lowercase());
    for b in tail.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/?#%[]".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    Some(Checked { url: out, host: host.to_string() })
}

/// The host of `url` when it may be opened ([`check`]).
pub fn openable(url: &str) -> Option<String> {
    check(url).map(|c| c.host)
}

/// A host label a browser reads as a number: decimal digits, or `0x` and hex digits.
fn numeric(label: &str) -> bool {
    let hex = label.strip_prefix("0x").is_some_and(|h| h.bytes().all(|b| b.is_ascii_hexdigit()));
    hex || label.bytes().all(|b| b.is_ascii_digit())
}

/// Four plain decimal parts of at most 255, without leading zeros: an IPv4 address as written.
fn dotted_quad(labels: &[&str]) -> bool {
    labels.len() == 4
        && labels.iter().all(|p| p.len() <= 3 && !(p.len() > 1 && p.starts_with('0')) && p.parse::<u8>().is_ok())
}

/// `host` in at most `max` characters: cut from its start (`…ample.test`), so the end that
/// says whose it is stays.
pub fn host_tail(host: &str, max: usize) -> String {
    let n = host.chars().count();
    if n <= max {
        return host.to_string();
    }
    let keep: String = host.chars().skip(n - max.saturating_sub(1)).collect();
    format!("…{keep}")
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
    let mut c = Command::new("/usr/bin/open");
    c.arg(url);
    c
}

#[cfg(windows)]
fn system(url: &str) -> Command {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let mut c = Command::new(std::path::Path::new(&root).join("System32").join("rundll32.exe"));
    c.arg("url.dll,FileProtocolHandler").arg(url);
    c
}

#[cfg(not(any(target_os = "macos", windows)))]
fn system(url: &str) -> Command {
    let mut c = Command::new("xdg-open");
    c.arg(url);
    c
}

/// Open `url` (checked again: [`check`], and handed over in its checked form) without waiting
/// for the opener, which is reaped on a thread of its own; its input and output go nowhere
/// (never into the terminal).
pub fn open(url: &str) -> io::Result<()> {
    let Some(checked) = check(url) else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not an http or https address"));
    };
    let mut child = command(&checked.url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
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
            &format!("https://{}.example.test/", "a".repeat(64)),
            &format!("https://{}example.test/", "abcdefghi.".repeat(25)),
            &format!("https://example.test/{}", "a".repeat(URL_MAX)),
            "http://0x7f.1/",
            "http://2130706433/",
            "http://127.000.0.1/",
            "http://example.256/",
            "https://exa%6dple.com/",
        ] {
            assert_eq!(openable(refused), None, "{refused:?}");
            assert!(open(refused).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn the_address_handed_over_is_normalised_and_hosts_keep_within_dns_limits() {
        let c = check("HTTPS://Docs.Example.com:8443/a b").map(|c| c.url);
        assert_eq!(c, None, "a space is refused");
        let c = check("HTTPS://Docs.Example.com:8443/caf\u{e9}/\"x\"|{y}^`z\\?q=1&r=%41#f").unwrap();
        assert_eq!(c.url, "https://docs.example.com:8443/caf%C3%A9/%22x%22%7C%7By%7D%5E%60z%5C?q=1&r=%41#f");
        assert_eq!(c.host, "docs.example.com");
        assert!(check("http://127.0.0.1:8080/").is_some(), "an IPv4 address as written opens");
        let longest = format!("{}.test", ["a".repeat(63), "b".repeat(63), "c".repeat(63), "d".repeat(56)].join("."));
        assert_eq!(longest.len(), 253);
        assert_eq!(openable(&format!("https://{longest}/")), Some(longest.clone()));
        assert_eq!(host_tail("docs.google.com.evil.test", 12), "…m.evil.test");
        assert_eq!(host_tail("example.test", 12), "example.test");
    }

    #[test]
    fn the_opener_is_handed_the_address_as_one_argument_without_a_shell() {
        let url = "https://example.com/a?b=1&c=$(touch x);d";
        let c = system(url);
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args.last().map(String::as_str), Some(url), "{args:?}");
        let program = c.get_program().to_string_lossy().into_owned();
        assert!(
            ["/usr/bin/open", "xdg-open"].contains(&program.as_str()) || program.ends_with("System32\\rundll32.exe"),
            "{program}"
        );
        assert!(!["sh", "bash", "cmd", "powershell"].contains(&program.as_str()));
    }
}
