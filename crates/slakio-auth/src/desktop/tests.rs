//! End-to-end import against a synthetic Slack profile: a fake Cookies database, a fake Local
//! Storage LevelDB and a fake `root-state.json`, all built in a temp directory, with a fake Safe
//! Storage key injected through the trait. No real Slack data, keychain or network is touched.

use super::*;
use aes::Aes128;
use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockEncryptMut, KeyIvInit};
use hmac::Hmac;
use rusty_leveldb::{DB, Options};
use sha1::Sha1;
use std::path::{Path, PathBuf};

type Enc = cbc::Encryptor<Aes128>;

const KEY: &[u8] = b"synthetic-safe-storage-key";

struct FakeStorage;
impl SafeStorage for FakeStorage {
    fn keys(&self, _sandboxed: bool) -> Result<Vec<Zeroizing<Vec<u8>>>, DesktopError> {
        Ok(vec![Zeroizing::new(KEY.to_vec())])
    }
}

/// A storage that declines (an empty list → no secret service), modelling a declined prompt.
struct DecliningStorage;
impl SafeStorage for DecliningStorage {
    fn keys(&self, _sandboxed: bool) -> Result<Vec<Zeroizing<Vec<u8>>>, DesktopError> {
        Ok(Vec::new())
    }
}

fn encrypt(plain: &str) -> Vec<u8> {
    let mut dk = [0u8; 16];
    pbkdf2::pbkdf2::<Hmac<Sha1>>(KEY, b"saltysalt", 1003, &mut dk).unwrap();
    let ct = Enc::new(&dk.into(), &[b' '; 16].into()).encrypt_padded_vec_mut::<Pkcs7>(plain.as_bytes());
    let mut out = b"v10".to_vec();
    out.extend_from_slice(&ct);
    out
}

fn scratch(tag: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let p = std::env::temp_dir().join(format!("slakio-import-{tag}-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Write a complete synthetic profile under `dir`: root-state.json, the cookies DB and the
/// Local Storage LevelDB, for the given team-id → (name, domain, token) triples.
fn write_profile(dir: &Path, teams: &[(&str, &str, &str, &str)]) {
    // root-state.json
    std::fs::create_dir_all(dir.join("storage")).unwrap();
    let ws: Vec<String> = teams
        .iter()
        .map(|(id, name, domain, _)| {
            format!(r#""{id}":{{"name":"{name}","url":"https://{domain}.slack.com/","domain":"{domain}","id":"{id}"}}"#)
        })
        .collect();
    std::fs::write(dir.join("storage/root-state.json"), format!(r#"{{"workspaces":{{{}}}}}"#, ws.join(","))).unwrap();

    // Cookies DB
    let conn = rusqlite::Connection::open(dir.join("Cookies")).unwrap();
    conn.execute_batch("CREATE TABLE cookies (host_key TEXT, name TEXT, value TEXT, encrypted_value BLOB);").unwrap();
    conn.execute("INSERT INTO cookies VALUES ('.slack.com','d','',?1)", [encrypt("fake-d-shared%2Fcookie")]).unwrap();
    conn.execute("INSERT INTO cookies VALUES ('.slack.com','d-s','',?1)", [encrypt("1-9999")]).unwrap();
    drop(conn);

    // Local Storage LevelDB
    let ldb = dir.join("Local Storage").join("leveldb");
    std::fs::create_dir_all(&ldb).unwrap();
    let mut db = DB::open(&ldb, Options { create_if_missing: true, ..Options::default() }).unwrap();
    let token_json: Vec<String> =
        teams.iter().map(|(id, _, _, tok)| format!(r#""{id}":{{"token":"{tok}"}}"#)).collect();
    let json = format!(r#"{{"teams":{{{}}}}}"#, token_json.join(","));
    let mut value = vec![0x01u8];
    value.extend_from_slice(json.as_bytes());
    db.put(b"_https://app.slack.com\x00\x01localConfig_v2", &value).unwrap();
    db.flush().unwrap();
    drop(db);
}

#[test]
fn imports_multiple_workspaces_with_credentials() {
    let dir = scratch("multi");
    write_profile(&dir, &[("T1", "Alpha", "alpha", "fake-c-alpha"), ("T2", "Beta", "beta", "fake-c-beta")]);
    let import = scan_dir(&dir).unwrap();

    let ws = import.workspaces();
    assert_eq!(ws.len(), 2);
    assert_eq!(ws[0].name, "Alpha"); // sorted by name
    assert_eq!(ws[0].host, "alpha.slack.com");

    let creds = import.credentials("T2", &FakeStorage).unwrap();
    assert_eq!(creds.token(), "fake-c-beta");
    assert_eq!(creds.cookie(), "fake-d-shared%2Fcookie");
    assert_eq!(creds.cookie_ds(), Some("1-9999"));
    assert_eq!(import.credentials("T404", &FakeStorage).unwrap_err(), DesktopError::NotSignedIn);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn only_workspaces_with_a_token_are_offered() {
    let dir = scratch("token-gated");
    // T2 is signed into root-state but has no LevelDB token → not offered.
    std::fs::create_dir_all(dir.join("storage")).unwrap();
    std::fs::write(
        dir.join("storage/root-state.json"),
        r#"{"workspaces":{"a":{"name":"A","domain":"a","id":"T1"},"b":{"name":"B","domain":"b","id":"T2"}}}"#,
    )
    .unwrap();
    let conn = rusqlite::Connection::open(dir.join("Cookies")).unwrap();
    conn.execute_batch("CREATE TABLE cookies (host_key TEXT, name TEXT, value TEXT, encrypted_value BLOB);").unwrap();
    conn.execute("INSERT INTO cookies VALUES ('.slack.com','d','',?1)", [encrypt("fake-d-c")]).unwrap();
    drop(conn);
    let ldb = dir.join("Local Storage").join("leveldb");
    std::fs::create_dir_all(&ldb).unwrap();
    let mut db = DB::open(&ldb, Options { create_if_missing: true, ..Options::default() }).unwrap();
    let mut value = vec![0x01u8];
    value.extend_from_slice(br#"{"teams":{"T1":{"token":"fake-c-1"}}}"#);
    db.put(b"x\x00\x01localConfig_v2", &value).unwrap();
    db.flush().unwrap();
    drop(db);

    let import = scan_dir(&dir).unwrap();
    assert_eq!(import.workspaces().len(), 1);
    assert_eq!(import.workspaces()[0].team, "T1");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn app_store_container_path_is_detected_as_sandboxed() {
    // A config dir under Library/Containers is the sandboxed App Store build.
    let base = scratch("appstore");
    let dir = base.join("Library/Containers/com.tinyspeck.slackmacgap/Data/Library/Application Support/Slack");
    std::fs::create_dir_all(&dir).unwrap();
    write_profile(&dir, &[("T1", "Store", "store", "fake-c-store")]);
    assert!(super::configdir::is_sandboxed(&dir));
    let import = scan_dir(&dir).unwrap();
    assert_eq!(import.workspaces()[0].team, "T1");
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn missing_profile_is_not_signed_in() {
    let dir = scratch("empty");
    assert_eq!(scan_dir(&dir).unwrap_err(), DesktopError::NotSignedIn);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn declined_keychain_yields_no_secret_service_without_leaking() {
    let dir = scratch("declined");
    write_profile(&dir, &[("T1", "Alpha", "alpha", "fake-c-alpha")]);
    let err = scan_dir(&dir).unwrap().credentials("T1", &DecliningStorage).unwrap_err();
    assert_eq!(err, DesktopError::NoSecretService);
    let shown = format!("{err}");
    assert!(!shown.contains("fake-c-") && !shown.contains("fake-d-"), "{shown}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn import_debug_and_error_never_print_secrets() {
    let dir = scratch("debug");
    write_profile(&dir, &[("T1", "Alpha", "alpha", "fake-c-alpha")]);
    let import = scan_dir(&dir).unwrap();
    let shown = format!("{import:?}");
    assert!(!shown.contains("fake-c-") && !shown.contains("fake-d-"), "{shown}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_scan_takes_no_key_source_and_a_missing_directory_is_not_found() {
    // `scan_dir` has no key source at all: the keychain prompt can only come with a pick.
    let dir = scratch("noprompt");
    write_profile(&dir, &[("T1", "Alpha", "alpha", "fake-c-alpha")]);
    let found = scan_dir(&dir).unwrap();
    assert_eq!(found.workspaces().len(), 1);
    assert_eq!(scan_dir(&dir.join("absent")).unwrap_err(), DesktopError::NotFound);
    std::fs::remove_dir_all(&dir).ok();
}

/// Counts the times the Safe Storage key is asked for (each one a keychain prompt).
struct CountingStorage(std::sync::atomic::AtomicUsize);
impl CountingStorage {
    fn asked(&self) -> usize {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}
impl SafeStorage for CountingStorage {
    fn keys(&self, _sandboxed: bool) -> Result<Vec<Zeroizing<Vec<u8>>>, DesktopError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(vec![Zeroizing::new(KEY.to_vec())])
    }
}

#[test]
fn credentials_kept_from_the_same_desktop_session_are_reused_without_the_safe_storage_key() {
    let dir = scratch("reuse");
    write_profile(&dir, &[("T1", "Alpha", "alpha", "fake-c-alpha"), ("T2", "Beta", "beta", "fake-c-beta")]);
    let found = scan_dir(&dir).unwrap();
    let storage = CountingStorage(std::sync::atomic::AtomicUsize::new(0));
    let first = found.credentials("T1", &storage).unwrap();
    assert_eq!(storage.asked(), 1, "the first sign-in reads the cookie: one prompt");
    // The next sign-in to the same workspace, with what the first one kept.
    let again = found.reuse("T1", Some(first.clone())).expect("the same session");
    assert_eq!((again.token(), again.cookie()), (first.token(), first.cookie()));
    assert_eq!(storage.asked(), 1, "no second prompt");
    // Kept from another session (the app signed in again since), another team, or nothing kept.
    let older = Credentials::new("fake-c-older", first.cookie());
    assert!(found.reuse("T1", Some(older)).is_none());
    assert!(found.reuse("T2", Some(first.clone())).is_none());
    assert!(found.reuse("T404", Some(first)).is_none());
    assert!(found.reuse("T1", None).is_none());
    std::fs::remove_dir_all(&dir).ok();
}
