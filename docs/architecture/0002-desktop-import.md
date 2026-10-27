# ADR 0002: reading the Slack desktop app's state for sign-in

Status: accepted (0.0.6).

## Context

Signing in by pasting a "Copy as cURL" request works but is fiddly. The Slack desktop app,
already signed in, holds everything a read-only client needs on disk: the session `d` cookie (in
an encrypted Chromium Cookies database, SQLite), the per-workspace `xoxc` tokens (in the app's
Local Storage, a Chromium LevelDB), and the signed-in workspace list (`storage/root-state.json`).
The reference client gammons/slk (MIT) reads exactly these. slakio adds the same import as the
first thing `slakio login` tries, falling back to the paste flow. This needs three readers slakio
did not have before: a SQLite reader, a LevelDB reader, and the AES-128-CBC + PBKDF2-SHA1 cookie
decryption.

`slakio-auth` must not depend on `slakio-slack` (dependency-direction CI): the import yields a
`Credentials` value, and the binary validates it through the transport (`client.userBoot`). macOS
is the only platform implemented in this release; Linux and Windows compile and report "not
supported yet, use --paste".

## Decision

In `slakio-auth`, macOS-only dependencies (`[target.'cfg(target_os = "macos")'.dependencies]`, so
no other platform pulls them and `cargo deny` leaves them out of its other-target graphs):

- **SQLite: `rusqlite` with the `bundled` feature.** The Cookies database is copied to a temp
  file and opened read-only (`SQLITE_OPEN_READ_ONLY`), as slk does, so the live profile is never
  locked or mutated. `bundled` pins the SQLite version so the build does not depend on a system
  library. This matches the already-planned `slakio-store` choice (step ②.3), so the two share
  one SQLite.
- **LevelDB: `rusty-leveldb` (`default-features = false`, `features = ["fs"]`).** A small,
  pure-Rust, still-maintained LevelDB reader — no C toolchain beyond SQLite's, no `unsafe` in
  slakio's own code (the workspace forbids it). `fs` selects the on-disk environment (the default
  with `default-features = false` is in-memory). The Local Storage directory is copied to a temp
  directory first and opened there.
- **Cookie decryption: the RustCrypto crates `aes`, `cbc`, `pbkdf2`, `hmac`, `sha1`.** Small,
  audited, permissively licensed building blocks for the exact scheme (PBKDF2-SHA1, salt
  `saltysalt`, 1003 rounds on macOS, a 16-space IV, AES-128-CBC), rather than a wrapper that
  hides the parameters.
- **JSON: `serde` / `serde_json`** (already in the workspace) for `root-state.json` and the
  `localConfig_v2` token blob.

The Safe Storage key is read behind a `SafeStorage` trait so tests inject a fake and never touch
the owner's keychain; the real macOS implementation shells out to `/usr/bin/security` (as slk
does), which is what raises the system's permission prompt.

The config-directory matrix, cookie query, keychain-account ordering, token layout and workspace
parsing are ported from gammons/slk `internal/slackdesktop`, whose decryption is adapted from
rneatherway/slack. Both MIT notices are retained in `NOTICE` and in the module headers.

## Alternatives

- **`leveldb`/`leveldb-sys` (the C LevelDB):** another C build and a system-library dependency;
  `rusty-leveldb` avoids both and reads the same on-disk format.
- **A hand-written LevelDB parser:** the format (MANIFEST + log + SST, with compression and a
  comparator) is exactly what a maintained crate is for; a minimal parser would be fragile across
  Chromium versions.
- **Linking the macOS Security framework** instead of shelling out to `security`: a C dependency
  for one keychain read, and the subprocess is what the reference client uses and what the owner
  can reason about in the permission prompt.

## Consequences

- Secrets live only in `Zeroizing` buffers and are wiped on drop; no secret reaches an error, a
  log line or a `Debug` print (the import's errors are fixed words). Fixture tests cover cookie
  decrypt, token discovery, multiple workspaces, the App Store container path, a missing app, a
  corrupt database, an unreadable (locked) database, a declined keychain and no-secret-in-error.
- The owner's real data is read only when the owner runs `slakio login` on their own machine;
  tests use synthetic fixtures built in temp directories.
- The new dependencies are macOS-only and covered by `cargo deny` / `cargo audit` and the
  generated third-party notices in CI.
- Caching the import result (so the keychain prompt is not raised on every start) is deferred to
  0.0.7.
