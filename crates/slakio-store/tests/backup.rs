//! `tmutil` run as a process, with a stand-in that records its arguments (Time Machine is never
//! touched). In a test binary of its own: a process started while another test thread lets go of
//! the cache's lock would hold that lock until it runs its program, so the tests that start
//! processes never share a process with the tests that open the cache.
#![cfg(unix)]

use slakio_store::{Exclude, Tmutil};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[test]
fn tmutil_is_run_with_addexclusion_and_the_directory_and_its_failure_is_an_error() {
    let dir = std::env::temp_dir().join(format!("slakio-backup-{}-tmutil", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // A stand-in for tmutil: records its arguments; fails when the directory says so.
    let program = dir.join("tmutil");
    let log = dir.join("args");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\ncase \"$2\" in *fail*) echo nope >&2; exit 1;; esac\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let tm = Tmutil { program: program.clone() };
    let ok = dir.join("ok");
    assert_eq!(tm.exclude(&ok), Ok(()));
    assert_eq!(std::fs::read_to_string(&log).unwrap(), format!("addexclusion\n{}\n", ok.display()));
    let err = tm.exclude(&dir.join("fail")).unwrap_err();
    assert!(err.contains("addexclusion") && err.contains("nope"), "{err}");
    let missing = Tmutil { program: dir.join("no-such-program") };
    assert!(missing.exclude(&ok).is_err());
    // A relative path is given absolute, never as one that reads as an option (`-p`).
    std::fs::remove_file(&log).unwrap();
    tm.exclude(Path::new("-p")).unwrap();
    let args = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = args.lines().collect();
    assert_eq!(lines.len(), 2, "{args}");
    assert!(lines[1].starts_with('/') && lines[1].ends_with("/-p"), "{args}");
}
