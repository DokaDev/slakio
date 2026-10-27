//! Decrypting a Chromium cookie value on macOS: AES-128-CBC with a PBKDF2-SHA1 key (salt
//! `saltysalt`, 1003 rounds, a 16-space IV). Ported from gammons/slk
//! `internal/slackdesktop/decrypt.go` (MIT), whose scheme is adapted from rneatherway/slack
//! (MIT). See `NOTICE`.
//!
//! The only cheap integrity check Chromium gives is PKCS#7 padding, and that is weak: the final
//! plaintext byte merely has to land in 1..=16, so a wrong Safe Storage key survives it about
//! one time in sixteen and yields binary garbage. [`looks_like_cookie_value`] closes that gap by
//! requiring printable ASCII, so a wrong key is rejected with probability (95/256)^len instead
//! of being sent on as an unusable `d` cookie.

use super::DesktopError;
use aes::Aes128;
use aes::cipher::block_padding::NoPadding;
use aes::cipher::{BlockDecryptMut, KeyIvInit};
use hmac::Hmac;
use sha1::Sha1;
use zeroize::Zeroizing;

type Aes128CbcDec = cbc::Decryptor<Aes128>;

const SALT: &[u8] = b"saltysalt";
/// macOS derives the key with 1003 PBKDF2 rounds (Linux uses 1; Windows is GCM, not built).
const ROUNDS_DARWIN: u32 = 1003;

/// The Chromium SHA-256 domain-hash prefixes that precede the decrypted value for
/// slack.com / .slack.com (cookie-DB version 24+); stripped when present.
const DOMAIN_HASH_PREFIXES: [[u8; 32]; 2] = [
    [
        3, 202, 236, 172, 132, 247, 212, 240, 217, 211, 68, 226, 103, 153, 245, 64, 85, 68, 2, 183, 83, 182, 186, 218,
        14, 102, 237, 62, 231, 241, 231, 142,
    ],
    [
        145, 28, 115, 68, 173, 92, 42, 78, 104, 243, 5, 63, 24, 206, 51, 190, 31, 169, 160, 244, 247, 106, 147, 228,
        60, 68, 92, 134, 105, 199, 162, 120,
    ],
];

fn strip_domain_hash_prefix(value: &[u8]) -> &[u8] {
    for p in &DOMAIN_HASH_PREFIXES {
        if let Some(rest) = value.strip_prefix(p.as_slice()) {
            return rest;
        }
    }
    value
}

/// A plausible decrypted cookie: non-empty and printable ASCII. See the module docs for why
/// padding alone is not enough of a guarantee.
fn looks_like_cookie_value(b: &[u8]) -> bool {
    !b.is_empty() && b.iter().all(|&c| (0x20..=0x7e).contains(&c))
}

/// Decrypt one `v10`/`v11` cookie value (its 3-byte version prefix already stripped) with
/// `password`, on macOS. Rejects a result that is not a plausible cookie (wrong key).
fn decrypt_cbc(value: &[u8], password: &[u8]) -> Result<Zeroizing<Vec<u8>>, DesktopError> {
    if value.is_empty() || !value.len().is_multiple_of(16) {
        return Err(DesktopError::DecryptFailed);
    }
    let mut dk = Zeroizing::new([0u8; 16]);
    pbkdf2::pbkdf2::<Hmac<Sha1>>(password, SALT, ROUNDS_DARWIN, dk.as_mut_slice())
        .map_err(|_| DesktopError::DecryptFailed)?;
    let iv = [b' '; 16];
    let mut buf = Zeroizing::new(value.to_vec());
    let out = Aes128CbcDec::new(dk.as_slice().into(), &iv.into())
        .decrypt_padded_mut::<NoPadding>(buf.as_mut_slice())
        .map_err(|_| DesktopError::DecryptFailed)?;
    // Strip PKCS#7 padding by the final byte only (Chromium does not guarantee every pad byte
    // equals the count); the printable check below is the real wrong-key guard.
    let n = *out.last().ok_or(DesktopError::DecryptFailed)? as usize;
    if n == 0 || n > 16 || n > out.len() {
        return Err(DesktopError::DecryptFailed);
    }
    let plain = Zeroizing::new(strip_domain_hash_prefix(&out[..out.len() - n]).to_vec());
    if !looks_like_cookie_value(&plain) {
        return Err(DesktopError::DecryptFailed);
    }
    Ok(plain)
}

/// Decrypt `enc` (a stored `encrypted_value` blob, version prefix included) by trying each key
/// in `keys` until one yields a plausible cookie. The first key is the most likely; a machine
/// with several Slack builds carries several keys and only trying them tells which belongs to
/// the profile.
pub(super) fn decrypt_cookie(enc: &[u8], keys: &[Zeroizing<Vec<u8>>]) -> Result<Zeroizing<String>, DesktopError> {
    if enc.len() < 3 {
        return Err(DesktopError::DecryptFailed);
    }
    let body = &enc[3..];
    if keys.is_empty() {
        return Err(DesktopError::NoSecretService);
    }
    let mut last = DesktopError::DecryptFailed;
    for key in keys {
        match decrypt_cbc(body, key) {
            Ok(plain) => {
                // The bytes are printable ASCII (checked above), so this is lossless.
                return Ok(Zeroizing::new(String::from_utf8_lossy(&plain).into_owned()));
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::{BlockEncryptMut, block_padding::Pkcs7};

    type Aes128CbcEnc = cbc::Encryptor<Aes128>;

    /// Encrypt `plain` the way Slack's Cookies DB stores it: `v10` + AES-128-CBC(PBKDF2(key)).
    fn encrypt(plain: &[u8], password: &[u8]) -> Vec<u8> {
        let mut dk = [0u8; 16];
        pbkdf2::pbkdf2::<Hmac<Sha1>>(password, SALT, ROUNDS_DARWIN, &mut dk).unwrap();
        let iv = [b' '; 16];
        let ct = Aes128CbcEnc::new(&dk.into(), &iv.into()).encrypt_padded_vec_mut::<Pkcs7>(plain);
        let mut out = b"v10".to_vec();
        out.extend_from_slice(&ct);
        out
    }

    #[test]
    fn round_trips_the_right_key() {
        let key = b"fake-safe-storage-key".to_vec();
        let enc = encrypt(b"fake-d-synthetic-cookie-value", &key);
        let got = decrypt_cookie(&enc, &[Zeroizing::new(key)]).unwrap();
        assert_eq!(got.as_str(), "fake-d-synthetic-cookie-value");
    }

    #[test]
    fn rejects_the_wrong_key_as_not_printable() {
        let enc = encrypt(b"fake-d-synthetic-cookie-value", b"the-real-key");
        let err = decrypt_cookie(&enc, &[Zeroizing::new(b"a-different-key".to_vec())]).unwrap_err();
        assert_eq!(err, DesktopError::DecryptFailed);
    }

    #[test]
    fn tries_every_key_until_one_works() {
        let key = b"second-key".to_vec();
        let enc = encrypt(b"fake-d-value", &key);
        let keys = vec![Zeroizing::new(b"first-wrong".to_vec()), Zeroizing::new(key)];
        assert_eq!(decrypt_cookie(&enc, &keys).unwrap().as_str(), "fake-d-value");
    }

    #[test]
    fn no_keys_is_no_secret_service() {
        assert_eq!(decrypt_cookie(b"v10anything", &[]).unwrap_err(), DesktopError::NoSecretService);
    }

    #[test]
    fn error_carries_no_cookie_text() {
        let enc = encrypt(b"fake-d-super-secret", b"real");
        let err = decrypt_cookie(&enc, &[Zeroizing::new(b"wrong".to_vec())]).unwrap_err();
        assert!(!format!("{err}").contains("fake-d-"));
    }
}
