//! The signed-in workspace list from the desktop app's `storage/root-state.json`. Ported from
//! gammons/slk `internal/slackdesktop/workspaces.go` (MIT).
//!
//! `root-state.json` holds `.workspaces{<k>: {name, url, domain, id}}`; entries with a non-empty
//! domain and id are kept and sorted by name. This gives the team id, the subdomain and the
//! display name with no network call — the list the sign-in picker shows.

use super::{DesktopError, DesktopWorkspace};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(serde::Deserialize)]
struct RootState {
    #[serde(default)]
    workspaces: BTreeMap<String, RawWorkspace>,
}

#[derive(serde::Deserialize)]
struct RawWorkspace {
    #[serde(default)]
    name: String,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    id: String,
}

/// Parse the signed-in workspaces from `root-state.json` bytes.
fn parse(data: &[u8]) -> Result<Vec<DesktopWorkspace>, DesktopError> {
    let rs: RootState = serde_json::from_slice(data).map_err(|_| DesktopError::NotSignedIn)?;
    let mut out: Vec<DesktopWorkspace> = rs
        .workspaces
        .into_values()
        .filter(|w| !w.domain.is_empty() && !w.id.is_empty())
        .map(|w| DesktopWorkspace { team: w.id, name: w.name, host: format!("{}.slack.com", w.domain) })
        .collect();
    if out.is_empty() {
        return Err(DesktopError::NotSignedIn);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// The largest `root-state.json` read (a real one is a few kilobytes).
const ROOT_STATE_MAX: u64 = 4 * 1024 * 1024;

/// Read and parse the desktop app's signed-in workspace list from `dir`. A file longer than
/// [`ROOT_STATE_MAX`] is refused ([`DesktopError::Io`]) without being read.
pub(super) fn workspaces(dir: &Path) -> Result<Vec<DesktopWorkspace>, DesktopError> {
    let data = super::read_capped(&dir.join("storage").join("root-state.json"), ROOT_STATE_MAX).map_err(|e| {
        if e.kind() == std::io::ErrorKind::FileTooLarge { DesktopError::Io } else { DesktopError::NotSignedIn }
    })?;
    parse(&data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_sorts_by_name() {
        let json = br#"{"workspaces":{
            "a":{"name":"Zeta","url":"https://zeta.slack.com/","domain":"zeta","id":"T2"},
            "b":{"name":"Alpha","url":"https://alpha.slack.com/","domain":"alpha","id":"T1"}
        }}"#;
        let got = parse(json).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(
            got[0],
            DesktopWorkspace { team: "T1".into(), name: "Alpha".into(), host: "alpha.slack.com".into() }
        );
        assert_eq!(got[1].team, "T2");
    }

    #[test]
    fn drops_entries_without_domain_or_id() {
        let json = br#"{"workspaces":{
            "a":{"name":"Has","domain":"has","id":"T1"},
            "b":{"name":"NoDomain","domain":"","id":"T2"},
            "c":{"name":"NoId","domain":"c","id":""}
        }}"#;
        let got = parse(json).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].team, "T1");
    }

    #[test]
    fn an_oversized_root_state_is_refused() {
        let dir = std::env::temp_dir().join(format!("slakio-ws-test-big-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("storage")).unwrap();
        let mut json = br#"{"workspaces":{"a":{"name":"A","domain":"a","id":"T1"}}}"#.to_vec();
        json.resize(json.len() + usize::try_from(ROOT_STATE_MAX).unwrap(), b' ');
        std::fs::write(dir.join("storage/root-state.json"), &json).unwrap();
        assert_eq!(workspaces(&dir).unwrap_err(), DesktopError::Io);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_workspaces_is_not_signed_in() {
        assert_eq!(parse(br#"{"workspaces":{}}"#).unwrap_err(), DesktopError::NotSignedIn);
        assert_eq!(parse(b"not json").unwrap_err(), DesktopError::NotSignedIn);
    }
}
