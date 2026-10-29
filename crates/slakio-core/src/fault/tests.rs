use super::*;

#[test]
fn toml_faults_know_their_line() {
    let text = "a = 1\nb = [\n";
    let e = text.parse::<toml_edit::DocumentMut>().unwrap_err();
    let f = Fault::toml_edit(text, &e);
    assert!(matches!(f.kind, FaultKind::Toml { line: Some(2 | 3) }), "{f:?}");
    assert!(!f.detail.is_empty());
    let e = "x = = 1".parse::<toml_edit::DocumentMut>().unwrap_err();
    assert_eq!(Fault::toml_edit("x = = 1", &e).kind, FaultKind::Toml { line: Some(1) });
}

#[test]
fn the_log_keeps_the_raw_detail_on_one_line() {
    let dir = std::env::temp_dir().join(format!("slakio-fault-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let log = ErrorLog::new(Some(dir.join("errors.log")));
    let e = io::Error::new(io::ErrorKind::PermissionDenied, "Permission denied (os error 13)");
    log.record("workspace.broken", &Fault::io_at(&e, Path::new("/x/workspace.toml")));
    log.record("config.error", &Fault::other("line one\nline two"));
    log.record("store.failed", &Fault::other(""));
    let text = std::fs::read_to_string(dir.join("errors.log")).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[2].ends_with("store.failed: Other (no detail given)"), "{text}");
    assert!(lines[0].contains("workspace.broken: Io(PermissionDenied): /x/workspace.toml: Permission denied"));
    assert!(lines[1].ends_with("config.error: Other: line one | line two"));
    ErrorLog::default().record("nowhere", &Fault::other("x"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Token- and cookie-shaped strings, spelled in parts so no file holds one.
fn token() -> String {
    ["xo", "xc-1234-5678-abcdefabcdef"].concat()
}
fn cookie() -> String {
    ["xo", "xd-AbCdEf%2Fgh%2Bij%3D%3D"].concat()
}

#[test]
fn no_token_cookie_address_past_its_host_or_quoted_text_reaches_the_log() {
    let (t, c) = (token(), cookie());
    let details = [
        format!(
            "curl 'https://a-team.slack.com/api/client.counts?_x_id=1' -H 'cookie: b=1; d={c}' --data-raw 'token={t}'"
        ),
        format!("Cookie: d={c}; d-s=1234 lc=1"),
        format!("authorization: Bearer {t}"),
        format!("token={t}&channel=C1"),
        format!("the sign-in {t} was refused"),
        "connect https://user:pw@files.example.test/private/T1-F1/secret.png?t=abc#x failed".to_string(),
        "invalid type: string \"lunch at noon with the team\", expected a map".to_string(),
        "unexpected `hello there` near 'private words".to_string(),
        "d=abc123; session=s3cr3tvalue".to_string(),
        format!("an opaque {} value", "Zq8".repeat(16)),
    ];
    for d in &details {
        let r = redact::redact(d);
        for leak in [
            t.as_str(),
            c.as_str(),
            "xoxc",
            "xoxd",
            "_x_id",
            "?t=",
            "pw@",
            "secret.png",
            "lunch",
            "hello",
            "private",
            "Bearer",
            "abc123",
            "s3cr3t",
            "Zq8Zq8",
        ] {
            assert!(!r.contains(leak), "{leak:?} in {r:?} (from {d:?})");
        }
    }
    assert_eq!(
        redact::redact("connect https://a-team.slack.com/api/x?y=1 failed"),
        "connect https://a-team.slack.com/… failed"
    );
    assert_eq!(redact::redact("token=abc&channel=C1"), "token=[secret]&channel=C1");
    // What says why stays.
    for kept in [
        "/x/workspace.toml: Permission denied (os error 13)",
        "invalid_auth",
        "HTTP 503",
        "don't panic",
        "retry after 30s",
    ] {
        assert_eq!(redact::redact(kept), kept);
    }
    let long = redact::redact(&"word ".repeat(200));
    assert!(long.chars().count() <= redact::LINE_MAX + 1 && long.ends_with('…'), "{long}");
}

#[test]
fn the_log_is_private_capped_and_rotated() {
    let dir = std::env::temp_dir().join(format!("slakio-fault-rot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("state").join("errors.log");
    let line = "x".repeat(100);
    for _ in 0..30 {
        append(&path, format!("{line}\n").as_bytes(), 1000).unwrap();
    }
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let old = dir.join("state").join("errors.log.1");
    assert!(size(&path) <= 1000 && size(&path) > 0, "{}", size(&path));
    assert!(size(&old) <= 1000 && size(&old) > 0, "{}", size(&old));
    assert!(!dir.join("state").join("errors.log.2").exists(), "two files at most");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&old), 0o600);
        assert_eq!(mode(&dir.join("state")), 0o700);
        // One an older version left readable is made private when next written.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        ErrorLog::new(Some(path.clone())).record("again", &Fault::other("x"));
        assert_eq!(mode(&path), 0o600);
        // A link is not written through.
        let target = dir.join("elsewhere");
        std::fs::write(&target, "").unwrap();
        let link = dir.join("state").join("linked.log");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        ErrorLog::new(Some(link)).record("linked", &Fault::other("x"));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
