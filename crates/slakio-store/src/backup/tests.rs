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

#[test]
fn only_the_caches_own_directory_is_excluded_never_the_one_named() {
    let dir = scratch("backup-own").join("named");
    let store = Store::open(&dir).unwrap().store;
    let fake = Fake { calls: RefCell::default(), fail: false };
    store.exclude_from_backups(&fake).unwrap();
    let excluded = fake.calls.borrow()[0].clone();
    assert!(excluded.is_absolute(), "{}", excluded.display());
    assert_eq!(excluded, std::path::absolute(dir.join(crate::db::DIR)).unwrap());
}
