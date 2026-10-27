//! The macOS "Slack Safe Storage" key source behind [`SafeStorage`]. Ported from gammons/slk
//! `internal/slackdesktop/password_darwin.go` (MIT).
//!
//! A login keychain can hold several items under this service name — one per Slack build ever
//! installed (the App Store build stores its key under a different account than the standalone
//! build) — and each key decrypts only its own profile's cookie DB. So the accounts are
//! enumerated from `security dump-keychain` (attributes only: no secret, no unlock prompt) and
//! ordered so the one matching the profile is tried first; then each key is read with
//! `security find-generic-password -w`. Each such read may raise the system's permission prompt,
//! which is why the matching account goes first — the common case needs a single read.

use super::{DesktopError, SafeStorage};
use std::process::Command;
use zeroize::Zeroizing;

const SERVICE: &str = "Slack Safe Storage";
const SECURITY: &str = "/usr/bin/security";

/// The real macOS login keychain. Shelling out to `/usr/bin/security` (rather than linking the
/// Security framework) keeps this free of a C dependency and matches how the reference client
/// reads the key.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemKeychain;

impl SafeStorage for SystemKeychain {
    fn keys(&self, sandboxed: bool) -> Result<Vec<Zeroizing<Vec<u8>>>, DesktopError> {
        let accounts = order_accounts(dump_accounts(), sandboxed);
        let mut out: Vec<Zeroizing<Vec<u8>>> = Vec::new();
        let mut seen: Vec<Vec<u8>> = Vec::new();
        for acct in &accounts {
            if let Some(pw) = find_password(Some(acct))
                && !seen.contains(&pw)
            {
                seen.push(pw.clone());
                out.push(Zeroizing::new(pw));
            }
        }
        if !out.is_empty() {
            return Ok(out);
        }
        // Enumeration told us nothing (dump-keychain unavailable or an unrecognised shape): fall
        // back to the service-only lookup, correct whenever only one item exists.
        match find_password(None) {
            Some(pw) => Ok(vec![Zeroizing::new(pw)]),
            None => Err(DesktopError::NoSecretService),
        }
    }
}

/// Read one secret. `security find-generic-password -w` prints just the password on stdout. No
/// account means "match on the service alone".
fn find_password(account: Option<&str>) -> Option<Vec<u8>> {
    let mut cmd = Command::new(SECURITY);
    cmd.args(["find-generic-password", "-w", "-s", SERVICE]);
    if let Some(a) = account {
        cmd.args(["-a", a]);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let mut s = out.stdout;
    while matches!(s.last(), Some(b'\n' | b'\r')) {
        s.pop();
    }
    (!s.is_empty()).then_some(s)
}

/// List the account names of every generic-password item with this service. `dump-keychain`
/// prints attributes only, never secret material, so it needs no unlock and raises no prompt.
fn dump_accounts() -> Vec<String> {
    let out = Command::new(SECURITY).arg("dump-keychain").output().ok();
    match out {
        Some(o) if o.status.success() => parse_accounts(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

/// Scan `security dump-keychain` output for items whose `svce` attribute equals the service and
/// return their `acct` values. Items begin with a `keychain:` line, and within one item the
/// attributes are printed alphabetically, so `acct` precedes `svce`.
fn parse_accounts(dump: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut acct = String::new();
    for line in dump.lines() {
        let t = line.trim_start();
        if t.starts_with("keychain:") {
            acct.clear();
        } else if t.starts_with("\"acct\"") {
            acct = attr_value(t);
        } else if t.starts_with("\"svce\"") && !acct.is_empty() && attr_value(t) == SERVICE {
            out.push(std::mem::take(&mut acct));
        }
    }
    out
}

/// Pull the quoted value out of a `dump-keychain` attribute line. Values come bare
/// (`"acct"<blob>="Slack Key"`) or hex-first with the text alongside
/// (`"acct"<blob>=0x…  "Slack"`); both end in the quoted form. `<NULL>` yields "".
fn attr_value(line: &str) -> String {
    let Some(eq) = line.find('=') else { return String::new() };
    let v = &line[eq + 1..];
    let (Some(first), Some(last)) = (v.find('"'), v.rfind('"')) else { return String::new() };
    if last <= first {
        return String::new();
    }
    v[first + 1..last].to_string()
}

/// Put the account most likely to match the profile first, so the common case needs a single
/// keychain read: the App Store account for a sandboxed profile, the standalone account
/// otherwise.
fn order_accounts(accounts: Vec<String>, sandboxed: bool) -> Vec<String> {
    let (mut matching, mut rest): (Vec<_>, Vec<_>) =
        accounts.into_iter().partition(|a| a.contains("App Store") == sandboxed);
    matching.append(&mut rest);
    matching
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_accounts_for_the_service() {
        let dump = concat!(
            "keychain: \"login.keychain-db\"\n",
            "    0x00000007 <blob>=\"Slack Safe Storage\"\n",
            "    \"acct\"<blob>=\"Slack Key\"\n",
            "    \"svce\"<blob>=\"Slack Safe Storage\"\n",
            "keychain: \"login\"\n",
            "    \"acct\"<blob>=\"Slack App Store Key\"\n",
            "    \"svce\"<blob>=\"Slack Safe Storage\"\n",
            "keychain: \"login\"\n",
            "    \"acct\"<blob>=\"Other\"\n",
            "    \"svce\"<blob>=\"Chrome Safe Storage\"\n",
        );
        assert_eq!(parse_accounts(dump), vec!["Slack Key", "Slack App Store Key"]);
    }

    #[test]
    fn hex_first_attribute_value() {
        assert_eq!(attr_value("    \"acct\"<blob>=0x536C61636B  \"Slack\""), "Slack");
        assert_eq!(attr_value("    \"acct\"<blob>=\"Plain\""), "Plain");
        assert_eq!(attr_value("    \"acct\"<blob>=<NULL>"), "");
    }

    #[test]
    fn sandboxed_profile_tries_app_store_account_first() {
        let accounts = vec!["Slack Key".to_string(), "Slack App Store Key".to_string()];
        assert_eq!(order_accounts(accounts.clone(), true), vec!["Slack App Store Key", "Slack Key"]);
        assert_eq!(order_accounts(accounts, false), vec!["Slack Key", "Slack App Store Key"]);
    }
}
