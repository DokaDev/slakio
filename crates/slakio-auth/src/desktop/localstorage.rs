//! The per-workspace session tokens from the desktop app's Local Storage (a Chromium LevelDB).
//! Ported from gammons/slk `internal/slackdesktop/localstorage.go` (MIT).
//!
//! Modern Slack no longer inlines the `xoxc` token in page HTML; it lives in the desktop app's
//! LevelDB under the `localConfig_v2` key, shaped `{teams: {<teamID>: {token}}}`. The LevelDB
//! directory is copied to a temp directory first (like the Cookies DB) so the live profile is
//! not locked or mutated while Slack is running.

use super::DesktopError;
use rusty_leveldb::{DB, LdbIterator, Options};
use std::collections::BTreeMap;
use std::path::Path;
use zeroize::Zeroizing;

const LOCAL_CONFIG_KEY: &str = "localConfig_v2";

#[derive(serde::Deserialize)]
struct LocalConfig {
    #[serde(default)]
    teams: BTreeMap<String, Team>,
}

#[derive(serde::Deserialize)]
struct Team {
    #[serde(default)]
    token: String,
}

/// A map of team id to session token from every `localConfig_v2` entry in the app's LevelDB.
/// [`DesktopError::NotSignedIn`] when none is found.
pub(super) fn tokens(dir: &Path) -> Result<BTreeMap<String, Zeroizing<String>>, DesktopError> {
    let ldb = dir.join("Local Storage").join("leveldb");
    if !ldb.is_dir() {
        return Err(DesktopError::NotSignedIn);
    }
    let tmp = copy_dir_to_temp(&ldb)?;
    read_tokens(tmp.path())
}

/// The LevelDB reader runs under `catch_unwind`: a damaged or hostile file that makes it panic
/// is reported as [`DesktopError::Io`] and never takes the program down.
fn read_tokens(db_dir: &Path) -> Result<BTreeMap<String, Zeroizing<String>>, DesktopError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_tokens_unguarded(db_dir)))
        .unwrap_or(Err(DesktopError::Io))
}

fn read_tokens_unguarded(db_dir: &Path) -> Result<BTreeMap<String, Zeroizing<String>>, DesktopError> {
    let opt = Options { create_if_missing: false, ..Options::default() };
    let mut db = DB::open(db_dir, opt).map_err(|_| DesktopError::Io)?;
    let mut iter = db.new_iter().map_err(|_| DesktopError::Io)?;
    let mut out: BTreeMap<String, Zeroizing<String>> = BTreeMap::new();
    iter.seek_to_first();
    while iter.valid() {
        let (mut key, mut val) = (Vec::new(), Zeroizing::new(Vec::new()));
        if iter.current(&mut key, &mut val) && key.ends_with(LOCAL_CONFIG_KEY.as_bytes()) {
            let json = Zeroizing::new(decode_ls_value(&val));
            if let Ok(cfg) = serde_json::from_str::<LocalConfig>(&json) {
                for (team, t) in cfg.teams {
                    if !t.token.is_empty() {
                        out.insert(team, Zeroizing::new(t.token));
                    }
                }
            }
        }
        iter.advance();
    }
    if out.is_empty() {
        return Err(DesktopError::NotSignedIn);
    }
    Ok(out)
}

/// Decode a Chromium localStorage value. Chromium prefixes the stored bytes with a one-byte
/// encoding tag: `0x00` → UTF-16LE, `0x01` → Latin-1 (used for all-ASCII values like this JSON).
/// An unknown or absent tag is returned as-is.
fn decode_ls_value(v: &[u8]) -> String {
    match v.first() {
        Some(0x00) => {
            let u16s: Zeroizing<Vec<u16>> =
                Zeroizing::new(v[1..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect());
            String::from_utf16_lossy(&u16s)
        }
        Some(0x01) => v[1..].iter().map(|&b| b as char).collect(),
        _ => String::from_utf8_lossy(v).into_owned(),
    }
}

/// The most read of the LevelDB directory: per file, and in all (a real one is a few megabytes).
const FILE_MAX: u64 = 64 * 1024 * 1024;
const TOTAL_MAX: u64 = 256 * 1024 * 1024;

/// Copy the regular files of `src` into a private temp directory (removed with them when
/// dropped). LevelDB directories are shallow (a handful of files), so the regular files are
/// enough. Each file is read into a buffer wiped when dropped (the values hold tokens).
fn copy_dir_to_temp(src: &Path) -> Result<tempfile::TempDir, DesktopError> {
    let dst = super::private_dir()?;
    let mut total = 0u64;
    for entry in std::fs::read_dir(src).map_err(|_| DesktopError::Io)? {
        let entry = entry.map_err(|_| DesktopError::Io)?;
        if entry.file_type().map_err(|_| DesktopError::Io)?.is_file() {
            let bytes = super::read_capped(&entry.path(), FILE_MAX).map_err(|_| DesktopError::Io)?;
            total += bytes.len() as u64;
            if total > TOTAL_MAX {
                return Err(DesktopError::Io);
            }
            std::fs::write(dst.path().join(entry.file_name()), bytes.as_slice()).map_err(|_| DesktopError::Io)?;
        }
    }
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let p = std::env::temp_dir().join(format!("slakio-ls-test-{tag}-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Build a synthetic Local Storage LevelDB holding one `localConfig_v2` entry. No real
    /// Slack data is involved.
    fn write_leveldb(dir: &Path, json: &str) {
        let ldb = dir.join("Local Storage").join("leveldb");
        std::fs::create_dir_all(&ldb).unwrap();
        let mut db = DB::open(&ldb, Options { create_if_missing: true, ..Options::default() }).unwrap();
        let key = b"_https://app.slack.com\x00\x01localConfig_v2";
        let mut value = vec![0x01u8]; // Latin-1 tag
        value.extend_from_slice(json.as_bytes());
        db.put(key, &value).unwrap();
        db.flush().unwrap();
        drop(db);
    }

    #[test]
    fn reads_one_workspace_token() {
        let dir = scratch("one");
        write_leveldb(&dir, r#"{"teams":{"T1":{"token":"fake-c-aaa"}}}"#);
        let got = tokens(&dir).unwrap();
        assert_eq!(got.get("T1").map(|t| t.to_string()), Some("fake-c-aaa".to_string()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_multiple_workspace_tokens_and_skips_empty() {
        let dir = scratch("many");
        write_leveldb(&dir, r#"{"teams":{"T1":{"token":"fake-c-1"},"T2":{"token":"fake-c-2"},"T3":{"token":""}}}"#);
        let got = tokens(&dir).unwrap();
        assert_eq!(got.len(), 2);
        assert!(got.contains_key("T1") && got.contains_key("T2"));
        assert!(!got.contains_key("T3"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_leveldb_is_not_signed_in() {
        let dir = scratch("absent");
        assert_eq!(tokens(&dir).unwrap_err(), DesktopError::NotSignedIn);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_local_config_entry_is_not_signed_in() {
        let dir = scratch("nocfg");
        let ldb = dir.join("Local Storage").join("leveldb");
        std::fs::create_dir_all(&ldb).unwrap();
        let mut db = DB::open(&ldb, Options { create_if_missing: true, ..Options::default() }).unwrap();
        db.put(b"_https://other\x00\x01unrelated", b"\x01{}").unwrap();
        db.flush().unwrap();
        drop(db);
        assert_eq!(tokens(&dir).unwrap_err(), DesktopError::NotSignedIn);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_corrupt_leveldb_is_an_error_not_a_panic() {
        let dir = scratch("corrupt");
        let ldb = dir.join("Local Storage").join("leveldb");
        std::fs::create_dir_all(&ldb).unwrap();
        std::fs::write(ldb.join("CURRENT"), b"MANIFEST-000002\n").unwrap();
        std::fs::write(ldb.join("MANIFEST-000002"), [0xffu8; 300]).unwrap();
        std::fs::write(ldb.join("000003.log"), [0x5au8; 4096]).unwrap();
        std::fs::write(ldb.join("000004.ldb"), b"not a table").unwrap();
        assert!(tokens(&dir).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn decodes_utf16le_values() {
        let mut v = vec![0x00u8];
        for u in "ab".encode_utf16() {
            v.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode_ls_value(&v), "ab");
    }
}
