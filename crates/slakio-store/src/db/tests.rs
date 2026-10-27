use super::*;
use crate::testutil::scratch;
use slakio_world::World;

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
#[test]
fn the_cache_is_private_to_the_user_also_where_it_was_not() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("perms").join("cache");
    // Left open by someone, or an older version: made private when opened.
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.join(LOCK), "").unwrap();
    std::fs::set_permissions(dir.join(LOCK), std::fs::Permissions::from_mode(0o644)).unwrap();
    let mut store = Store::open(&dir).unwrap().store;
    store.put_snapshot(World::new(3).snapshot()).unwrap();
    assert_eq!(mode(&dir), 0o700, "the directory");
    for name in [FILE, "cache.sqlite3-wal", "cache.sqlite3-shm", LOCK] {
        assert_eq!(mode(&dir.join(name)), 0o600, "{name}");
    }
    drop(store);
    std::fs::set_permissions(dir.join(FILE), std::fs::Permissions::from_mode(0o644)).unwrap();
    let _store = Store::open(&dir).unwrap().store;
    assert_eq!(mode(&dir.join(FILE)), 0o600, "a database made readable is made private again");
}

#[test]
fn a_second_process_reads_the_cache_but_never_writes_it() {
    let dir = scratch("lock").join("cache");
    let world = World::new(3);
    let ids: Vec<_> = world.snapshot().workspaces.iter().map(|w| w.id.clone()).collect();
    let mut first = Store::open(&dir).unwrap().store;
    assert!(!first.read_only());
    first.put_snapshot(world.snapshot()).unwrap();
    let mut second = Store::open(&dir).unwrap().store;
    assert!(second.read_only(), "the lock is held: read-only");
    assert_eq!(second.snapshot(&ids).unwrap().as_ref(), Some(world.snapshot()), "it reads what the first wrote");
    // Its writes are nothing.
    second.put_snapshot(World::new(4).snapshot()).unwrap();
    second.set_meta("k", "v").unwrap();
    assert!(second.rebuild().is_err());
    assert_eq!(first.snapshot(&ids).unwrap().as_ref(), Some(world.snapshot()));
    assert_eq!(first.meta("k").unwrap(), None);
    drop(second);
    drop(first);
    let third = Store::open(&dir).unwrap().store;
    assert!(!third.read_only(), "the lock went with the first");
}

#[test]
fn a_second_process_with_nothing_to_read_does_without() {
    let dir = scratch("lock-empty").join("cache");
    // Another process holds the lock and has written nothing yet.
    std::fs::create_dir_all(&dir).unwrap();
    let held = File::create(dir.join(LOCK)).unwrap();
    held.try_lock().unwrap();
    assert!(matches!(Store::open(&dir), Err(OpenError::InUse)));
    assert!(!dir.join(FILE).exists(), "nothing is made by the reader");
    drop(held);
    assert!(!Store::open(&dir).unwrap().store.read_only());
}

#[test]
fn a_cache_that_is_not_a_database_is_set_up_again() {
    for (tag, junk) in [
        ("junk", b"this is not a database at all, not even close".repeat(200)),
        ("header", {
            let mut h = b"SQLite format 3\0".to_vec();
            h.extend_from_slice(&[0xFF; 84]);
            h
        }),
    ] {
        let dir = scratch(tag).join("cache");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), &junk).unwrap();
        let opened = Store::open(&dir).unwrap();
        assert!(opened.rebuilt, "{tag}");
        let mut store = opened.store;
        store.put_snapshot(World::new(3).snapshot()).unwrap();
        drop(store);
        assert!(!Store::open(&dir).unwrap().rebuilt, "{tag}: once rebuilt it opens as it is");
    }
}

#[test]
fn a_cache_of_a_newer_version_is_set_up_again() {
    let dir = scratch("newer").join("cache");
    let store = Store::open(&dir).unwrap().store;
    store.conn.execute("UPDATE schema_version SET version = 999", []).unwrap();
    store.set_meta("old", "x").unwrap();
    drop(store);
    let opened = Store::open(&dir).unwrap();
    assert!(opened.rebuilt);
    assert_eq!(opened.store.meta("old").unwrap(), None, "nothing of the newer one is read");
    assert_eq!(schema::version(&opened.store.conn).unwrap() as usize, MIGRATIONS.len());
}

#[test]
fn a_cache_rebuilt_while_in_use_is_empty_and_still_written() {
    let dir = scratch("rebuild").join("cache");
    let world = World::new(3);
    let ids: Vec<_> = world.snapshot().workspaces.iter().map(|w| w.id.clone()).collect();
    let mut store = Store::open(&dir).unwrap().store;
    store.put_snapshot(world.snapshot()).unwrap();
    store.rebuild().unwrap();
    assert_eq!(store.snapshot(&ids).unwrap(), None);
    store.put_snapshot(world.snapshot()).unwrap();
    assert!(store.snapshot(&ids).unwrap().is_some());
    assert!(!store.read_only());
}

#[test]
fn a_directory_that_cannot_be_made_is_a_fault() {
    let dir = scratch("nodir");
    let file = dir.join("a-file");
    std::fs::write(&file, "x").unwrap();
    assert!(matches!(Store::open(&file.join("cache")), Err(OpenError::Failed(_))));
}

#[test]
fn removing_the_cache_leaves_nothing_of_it() {
    let dir = scratch("remove").join("cache");
    let mut store = Store::open(&dir).unwrap().store;
    store.put_snapshot(World::new(3).snapshot()).unwrap();
    drop(store);
    remove(&dir).unwrap();
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        0,
        "{:?}",
        std::fs::read_dir(&dir).unwrap().collect::<Vec<_>>()
    );
    remove(&dir).unwrap();
}
