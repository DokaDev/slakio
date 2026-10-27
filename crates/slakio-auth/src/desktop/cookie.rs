//! Reading and decrypting the Slack desktop `d` cookie (and `d-s`, when present) from the app's
//! Cookies database. Ported from gammons/slk `internal/slackdesktop/cookie.go` and
//! `cookiedb.go` (MIT).
//!
//! Slack keeps the Cookies database open (often in WAL mode) while it runs, so it is copied to a
//! temp file and the copy is read: the live profile is never locked, nor are `-wal`/`-shm` side
//! files written into it. The `d` cookie is written at login and long since checkpointed, so the
//! main-file snapshot is complete.

use super::decrypt::decrypt_cookie;
use super::{DesktopError, SafeStorage};
use std::path::Path;
use zeroize::Zeroizing;

const QUERY: &str = "SELECT value, encrypted_value FROM cookies WHERE host_key = '.slack.com' AND name = ?1";

/// The `d` cookie (required) and the `d-s` cookie (optional) for `dir`, decrypted. The keychain
/// keys are read once (one permission prompt) and reused for both.
pub(super) fn cookies(
    dir: &Path,
    storage: &dyn SafeStorage,
    sandboxed: bool,
) -> Result<(Zeroizing<String>, Option<Zeroizing<String>>), DesktopError> {
    let db_path = dir.join("Cookies");
    if !db_path.is_file() {
        return Err(DesktopError::CookieMissing);
    }
    let tmp = copy_to_temp(&db_path)?;
    let conn = rusqlite::Connection::open_with_flags(
        tmp.path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| DesktopError::Io)?;

    let d = read_row(&conn, "d")?.ok_or(DesktopError::CookieMissing)?;
    // The keys can prompt, so read them once, only after we know there is a `d` cookie to use.
    let keys = storage.keys(sandboxed)?;
    let cookie = value_of(d, &keys)?;
    let cookie_ds = match read_row(&conn, "d-s")? {
        Some(row) => value_of(row, &keys).ok(),
        None => None,
    };
    Ok((cookie, cookie_ds))
}

/// A cookie row: the plaintext value (usually empty) and the encrypted blob.
struct Row {
    plain: String,
    enc: Vec<u8>,
}

fn read_row(conn: &rusqlite::Connection, name: &str) -> Result<Option<Row>, DesktopError> {
    let mut stmt = conn.prepare(QUERY).map_err(|_| DesktopError::Io)?;
    let row = stmt
        .query_row([name], |r| {
            Ok(Row {
                plain: r.get::<_, String>(0).unwrap_or_default(),
                enc: r.get::<_, Vec<u8>>(1).unwrap_or_default(),
            })
        })
        .map(Some)
        .or_else(
            |e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(DesktopError::Io) },
        )?;
    Ok(row)
}

/// The usable value of a row: the plaintext when Slack stored one, otherwise the decrypted blob.
fn value_of(row: Row, keys: &[Zeroizing<Vec<u8>>]) -> Result<Zeroizing<String>, DesktopError> {
    if !row.plain.is_empty() {
        return Ok(Zeroizing::new(row.plain));
    }
    decrypt_cookie(&row.enc, keys)
}

/// A removed-on-drop copy of `src`.
struct TempFile(std::path::PathBuf);

impl TempFile {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn copy_to_temp(src: &Path) -> Result<TempFile, DesktopError> {
    let bytes = match std::fs::read(src) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Err(DesktopError::CookieLocked),
        Err(_) => return Err(DesktopError::CookieMissing),
    };
    let name = format!("slakio-cookies-{}-{}.db", std::process::id(), nonce());
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, &bytes).map_err(|_| DesktopError::Io)?;
    Ok(TempFile(path))
}

/// A small per-call nonce so two imports in the same process do not collide.
fn nonce() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake Safe Storage that returns one fixed key (never the owner's keychain).
    struct FakeStorage(Vec<u8>);
    impl SafeStorage for FakeStorage {
        fn keys(&self, _sandboxed: bool) -> Result<Vec<Zeroizing<Vec<u8>>>, DesktopError> {
            Ok(vec![Zeroizing::new(self.0.clone())])
        }
    }

    use aes::Aes128;
    use aes::cipher::block_padding::Pkcs7;
    use aes::cipher::{BlockEncryptMut, KeyIvInit};
    use hmac::Hmac;
    use sha1::Sha1;
    type Enc = cbc::Encryptor<Aes128>;

    fn encrypt(plain: &[u8], password: &[u8]) -> Vec<u8> {
        let mut dk = [0u8; 16];
        pbkdf2::pbkdf2::<Hmac<Sha1>>(password, b"saltysalt", 1003, &mut dk).unwrap();
        let ct = Enc::new(&dk.into(), &[b' '; 16].into()).encrypt_padded_vec_mut::<Pkcs7>(plain);
        let mut out = b"v10".to_vec();
        out.extend_from_slice(&ct);
        out
    }

    /// Build a synthetic Cookies DB in `dir` with the given `d` (and optional `d-s`) encrypted
    /// under `key`. No real Slack data is involved.
    fn write_db(dir: &Path, key: &[u8], d: Option<&str>, ds: Option<&str>) {
        let conn = rusqlite::Connection::open(dir.join("Cookies")).unwrap();
        conn.execute_batch("CREATE TABLE cookies (host_key TEXT, name TEXT, value TEXT, encrypted_value BLOB);")
            .unwrap();
        if let Some(d) = d {
            conn.execute("INSERT INTO cookies VALUES ('.slack.com','d','',?1)", [encrypt(d.as_bytes(), key)]).unwrap();
        }
        if let Some(ds) = ds {
            conn.execute("INSERT INTO cookies VALUES ('.slack.com','d-s','',?1)", [encrypt(ds.as_bytes(), key)])
                .unwrap();
        }
    }

    fn scratch() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("slakio-cookie-test-{}-{}", std::process::id(), nonce()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn reads_and_decrypts_d_and_ds() {
        let dir = scratch();
        let key = b"k".to_vec();
        write_db(&dir, &key, Some("fake-d-abc%2F123"), Some("1-234"));
        let (d, ds) = cookies(&dir, &FakeStorage(key), false).unwrap();
        assert_eq!(d.as_str(), "fake-d-abc%2F123");
        assert_eq!(ds.map(|s| s.to_string()), Some("1-234".to_string()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn d_only_leaves_ds_none() {
        let dir = scratch();
        let key = b"k".to_vec();
        write_db(&dir, &key, Some("fake-d-only"), None);
        let (d, ds) = cookies(&dir, &FakeStorage(key), false).unwrap();
        assert_eq!(d.as_str(), "fake-d-only");
        assert!(ds.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_db_is_cookie_missing() {
        let dir = scratch();
        assert_eq!(cookies(&dir, &FakeStorage(b"k".to_vec()), false).unwrap_err(), DesktopError::CookieMissing);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn db_without_a_d_cookie_is_cookie_missing() {
        let dir = scratch();
        write_db(&dir, b"k", None, None);
        assert_eq!(cookies(&dir, &FakeStorage(b"k".to_vec()), false).unwrap_err(), DesktopError::CookieMissing);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_db_is_an_io_error_not_a_panic() {
        let dir = scratch();
        std::fs::write(dir.join("Cookies"), b"this is not a sqlite database").unwrap();
        assert_eq!(cookies(&dir, &FakeStorage(b"k".to_vec()), false).unwrap_err(), DesktopError::Io);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unreadable_db_is_reported_as_locked() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch();
        let path = dir.join("Cookies");
        std::fs::write(&path, b"x").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let err = cookies(&dir, &FakeStorage(b"k".to_vec()), false).unwrap_err();
        // Restore so the directory can be removed.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).ok();
        assert_eq!(err, DesktopError::CookieLocked);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wrong_key_fails_to_decrypt_without_leaking() {
        let dir = scratch();
        write_db(&dir, b"real-key", Some("fake-d-secret"), None);
        let err = cookies(&dir, &FakeStorage(b"wrong-key".to_vec()), false).unwrap_err();
        assert_eq!(err, DesktopError::DecryptFailed);
        assert!(!format!("{err}").contains("fake-d-"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
