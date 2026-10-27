use super::*;
use crate::testutil::{scratch, writer};
use std::cell::RefCell;

/// Records each directory it is asked to exclude, and fails when told to.
struct Fake {
    calls: RefCell<Vec<PathBuf>>,
    fail: bool,
}

impl Exclude for Fake {
    fn exclude(&self, dir: &Path) -> Result<(), String> {
        self.calls.borrow_mut().push(dir.to_path_buf());
        if self.fail { Err("refused".into()) } else { Ok(()) }
    }
}

#[test]
fn the_cache_directory_is_excluded_once_and_remembered() {
    let store = writer("backup");
    let fake = Fake { calls: RefCell::default(), fail: false };
    assert_eq!(store.exclude_from_backups(&fake), Ok(true));
    assert_eq!(*fake.calls.borrow(), [store.dir().to_path_buf()]);
    assert_eq!(store.exclude_from_backups(&fake), Ok(false), "remembered: not asked again");
    assert_eq!(fake.calls.borrow().len(), 1);
}

#[test]
fn a_failed_exclusion_is_said_and_tried_again_next_time() {
    let store = writer("backup-fail");
    let fake = Fake { calls: RefCell::default(), fail: true };
    assert!(store.exclude_from_backups(&fake).is_err());
    assert!(store.exclude_from_backups(&fake).is_err());
    assert_eq!(fake.calls.borrow().len(), 2, "not remembered as done");
}

#[test]
fn a_read_only_cache_asks_nothing() {
    let dir = scratch("backup-ro").join("cache");
    let _writer = Store::open(&dir).unwrap().store;
    let reader = Store::open(&dir).unwrap().store;
    let fake = Fake { calls: RefCell::default(), fail: false };
    assert_eq!(reader.exclude_from_backups(&fake), Ok(false));
    assert!(fake.calls.borrow().is_empty());
}

#[cfg(unix)]
#[test]
fn tmutil_is_run_with_addexclusion_and_the_directory_and_its_failure_is_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("tmutil");
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
}
