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
    let named = scratch("perms").join("cache");
    let dir = named.join(DIR);
    // Left open by someone, or an older version: made private when opened.
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&named, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.join(LOCK), "").unwrap();
    std::fs::set_permissions(dir.join(LOCK), std::fs::Permissions::from_mode(0o644)).unwrap();
    let mut store = Store::open(&named).unwrap().store;
    store.put_snapshot(World::new(3).snapshot()).unwrap();
    assert_eq!(mode(&dir), 0o700, "the cache's own directory");
    assert_eq!(mode(&named), 0o755, "the directory named is left as it is");
    for name in [FILE, "cache.sqlite3-wal", "cache.sqlite3-shm", LOCK] {
        assert_eq!(mode(&dir.join(name)), 0o600, "{name}");
    }
    drop(store);
    std::fs::set_permissions(dir.join(FILE), std::fs::Permissions::from_mode(0o644)).unwrap();
    let _store = Store::open(&named).unwrap().store;
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
    let read = second.snapshot(&ids[..1]).unwrap();
    assert_eq!(read, Some(world.snapshot().only(&ids[0])), "it reads what the first wrote");
    // Its writes are nothing.
    second.put_snapshot(World::new(4).snapshot()).unwrap();
    second.set_meta("k", "v").unwrap();
    assert!(second.rebuild().is_err());
    assert_eq!(first.snapshot(&ids[..1]).unwrap(), Some(world.snapshot().only(&ids[0])));
    assert_eq!(first.meta("k").unwrap(), None);
    drop(second);
    drop(first);
    let third = Store::open(&dir).unwrap().store;
    assert!(!third.read_only(), "the lock went with the first");
}

#[test]
fn a_second_process_with_nothing_to_read_does_without() {
    let named = scratch("lock-empty").join("cache");
    let dir = named.join(DIR);
    // Another process holds the lock and has written nothing yet.
    std::fs::create_dir_all(&dir).unwrap();
    let held = File::create(dir.join(LOCK)).unwrap();
    held.try_lock().unwrap();
    assert!(in_use(&named), "a slakio uses it");
    assert!(matches!(Store::open(&named), Err(OpenError::InUse)));
    assert!(!dir.join(FILE).exists(), "nothing is made by the reader");
    drop(held);
    assert!(!in_use(&named));
    assert!(!Store::open(&named).unwrap().store.read_only());
}

#[test]
fn a_lock_let_go_a_moment_later_is_taken_not_read_only() {
    let named = scratch("lock-retry").join("cache");
    let dir = named.join(DIR);
    // Held, as by a process being started that shares the lock's open file, then let go.
    std::fs::create_dir_all(&dir).unwrap();
    let held = File::create(dir.join(LOCK)).unwrap();
    held.try_lock().unwrap();
    let letting_go = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        drop(held);
    });
    // A wait longer than the default, so a slow machine's late wake-up still falls inside it.
    let store = Store::open_waiting(&named, Duration::from_secs(10)).unwrap().store;
    letting_go.join().unwrap();
    assert!(!store.read_only(), "the lock let go within the wait is taken");
    drop(store);
    // Never let go within the wait: read-only, after trying for the wait.
    let held = File::create(dir.join(LOCK)).unwrap();
    held.try_lock().unwrap();
    let started = Instant::now();
    assert!(Store::open(&named).unwrap().store.read_only());
    assert!(started.elapsed() >= LOCK_WAIT, "{:?}", started.elapsed());
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
        let named = scratch(tag).join("cache");
        let dir = named.join(DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), &junk).unwrap();
        let opened = Store::open(&named).unwrap();
        assert!(opened.rebuilt, "{tag}");
        let mut store = opened.store;
        store.put_snapshot(World::new(3).snapshot()).unwrap();
        drop(store);
        assert!(!Store::open(&named).unwrap().rebuilt, "{tag}: once rebuilt it opens as it is");
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
fn a_cache_rebuilt_while_in_use_is_still_the_sign_ins() {
    let named = scratch("rebuild-owner").join("cache");
    let world = World::new(3);
    let ids: Vec<_> = world.snapshot().workspaces.iter().map(|w| w.id.clone()).collect();
    let (a, b) = (ids[0].as_str(), ids[1].as_str());
    let mut store = Store::open(&named).unwrap().store;
    assert!(store.claim(a, "UA").unwrap() && store.claim(b, "UB").unwrap());
    store.rebuild().unwrap();
    assert_eq!(store.meta(&format!("{OWNER}{a}")).unwrap().as_deref(), Some("UA"));
    assert_eq!(store.meta(&format!("{OWNER}{b}")).unwrap().as_deref(), Some("UB"), "every workspace's owner");
    store.put_snapshot(world.snapshot()).unwrap();
    // A second slakio of the same sign-in meanwhile uses it.
    let mut reader = Store::open(&named).unwrap().store;
    assert!(reader.read_only() && reader.claim(a, "UA").unwrap());
    drop(reader);
    drop(store);
    // The next start keeps what the run wrote after the rebuild.
    let mut store = Store::open(&named).unwrap().store;
    assert!(store.claim(a, "UA").unwrap() && store.claim(b, "UB").unwrap());
    assert!(store.snapshot(&ids).unwrap().is_some());
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
    std::fs::write(dir.join("kept"), "the user's").unwrap();
    // A development version's files, kept in the cache directory itself.
    for name in [FILE, "cache.sqlite3-wal", "cache.sqlite3-shm", LOCK] {
        std::fs::write(dir.join(name), "old").unwrap();
    }
    remove(&dir).unwrap();
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(left, ["kept"], "nothing of the cache is left, and nothing else goes");
    remove(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_in_place_of_the_cache_or_its_files_is_refused() {
    use std::os::unix::fs::symlink;
    let elsewhere = scratch("links-target");
    std::fs::write(elsewhere.join("victim"), "x").unwrap();
    // The cache's own directory a link.
    let named = scratch("links-dir").join("cache");
    std::fs::create_dir_all(&named).unwrap();
    symlink(&elsewhere, named.join(DIR)).unwrap();
    assert!(matches!(Store::open(&named), Err(OpenError::Failed(_))));
    assert!(!elsewhere.join(FILE).exists(), "nothing made through it");
    // The database, the lock, a journal file a link: refused, the file it names untouched.
    for name in [FILE, LOCK, "cache.sqlite3-wal", "cache.sqlite3-shm"] {
        let named = scratch(&format!("links-{name}")).join("cache");
        let dir = named.join(DIR);
        if name.ends_with("-wal") || name.ends_with("-shm") {
            drop(Store::open(&named).unwrap());
            let _ = std::fs::remove_file(dir.join(name));
        } else {
            std::fs::create_dir_all(&dir).unwrap();
        }
        let victim = elsewhere.join("victim");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o644)).unwrap();
        symlink(&victim, dir.join(name)).unwrap();
        assert!(matches!(Store::open(&named), Err(OpenError::Failed(_))), "{name}");
        assert_eq!(mode(&victim), 0o644, "{name}: what the link names is not changed");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "x");
    }
}

#[test]
fn each_workspaces_part_is_one_sign_ins_and_another_finds_it_empty() {
    let named = scratch("owner").join("cache");
    let world = World::new(3);
    let ids: Vec<_> = world.snapshot().workspaces.iter().map(|w| w.id.clone()).collect();
    let (a, b) = (ids[0].as_str(), ids[1].as_str());
    let mut store = Store::open(&named).unwrap().store;
    assert!(store.claim(a, "UA").unwrap() && store.claim(b, "UB").unwrap());
    store.put_snapshot(world.snapshot()).unwrap();
    assert!(store.claim(a, "UA").unwrap(), "the same sign-in keeps it");
    assert!(store.snapshot(&ids).unwrap().is_some());
    // Another process while this one writes: another user's part is not to be used.
    let mut reader = Store::open(&named).unwrap().store;
    assert!(reader.read_only());
    assert!(!reader.claim(a, "UX").unwrap());
    assert!(reader.claim(a, "UA").unwrap());
    drop(reader);
    drop(store);
    // Another user of workspace A signed in: nothing of the first one's A is read; B's part,
    // another sign-in's, is left as it was.
    let mut store = Store::open(&named).unwrap().store;
    assert!(store.claim(a, "UX").unwrap());
    assert_eq!(store.snapshot(&ids[..1]).unwrap(), None);
    assert!(store.snapshot(&ids[1..]).unwrap().is_some(), "B is kept");
    assert_eq!(store.meta(&format!("{OWNER}{a}")).unwrap().as_deref(), Some("UX"));
    assert_eq!(store.meta(&format!("{OWNER}{b}")).unwrap().as_deref(), Some("UB"));
    drop(store);
    // A part that does not say whose it is: emptied too, and only it.
    let mut store = Store::open(&scratch("owner-none").join("cache")).unwrap().store;
    store.put_snapshot(world.snapshot()).unwrap();
    assert!(store.claim(a, "UA").unwrap());
    assert_eq!(store.snapshot(&ids[..1]).unwrap(), None);
    assert!(store.snapshot(&ids[1..]).unwrap().is_some());
}

#[test]
fn signing_out_of_one_workspace_removes_its_part_only() {
    let named = scratch("forget").join("cache");
    let world = World::new(3);
    let ids: Vec<_> = world.snapshot().workspaces.iter().map(|w| w.id.clone()).collect();
    let (a, b) = (ids[0].as_str(), ids[1].as_str());
    let mut store = Store::open(&named).unwrap().store;
    assert!(store.claim(a, "UA").unwrap() && store.claim(b, "UB").unwrap());
    store.put_snapshot(world.snapshot()).unwrap();
    let c = world.snapshot().conversations.iter().find(|c| c.workspace.as_str() == a).unwrap();
    let t = slakio_core::model::Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() };
    let (messages, _) = world.history(&c.id, None, 5);
    let page = slakio_core::backend::Page { target: t.clone(), messages, complete: false, users: vec![] };
    store.put_page(&page, None, 1).unwrap();
    let mut reader = Store::open(&named).unwrap().store;
    assert!(reader.forget(a).is_err(), "a reader removes nothing");
    drop(reader);
    store.forget(a).unwrap();
    assert_eq!(store.snapshot(&ids[..1]).unwrap(), None);
    assert_eq!(store.newest_page(&t, 5).unwrap(), None, "its messages went too");
    assert_eq!(store.meta(&format!("{OWNER}{a}")).unwrap(), None);
    assert!(store.snapshot(&ids[1..]).unwrap().is_some());
    assert_eq!(store.meta(&format!("{OWNER}{b}")).unwrap().as_deref(), Some("UB"));
    // Set up again later: A is no one's any more.
    store.rebuild().unwrap();
    assert_eq!(store.meta(&format!("{OWNER}{a}")).unwrap(), None);
}

#[test]
fn the_journal_is_wal_and_a_file_system_without_it_is_not_used() {
    let store = Store::open(&scratch("wal").join("cache")).unwrap().store;
    let mode: String = store.conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
    assert!(wal("wal").is_ok());
    let memory = Connection::open_in_memory().unwrap();
    let mode: String = memory.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0)).unwrap();
    assert!(wal(&mode).is_err(), "{mode}: refused");
}

#[test]
fn a_claim_that_could_not_empty_a_part_leaves_it_the_sign_in_it_was() {
    let named = scratch("claim-failed").join("cache");
    let world = World::new(3);
    let ids: Vec<_> = world.snapshot().workspaces.iter().map(|w| w.id.clone()).collect();
    let a = ids[0].as_str();
    let mut store = Store::open(&named).unwrap().store;
    assert!(store.claim(a, "UA").unwrap());
    store.put_snapshot(world.snapshot()).unwrap();
    // Nothing can be written: emptying it for another sign-in fails.
    store.conn.pragma_update(None, "query_only", true).unwrap();
    assert!(store.claim(a, "UB").is_err());
    store.conn.pragma_update(None, "query_only", false).unwrap();
    assert_eq!(store.meta(&format!("{OWNER}{a}")).unwrap().as_deref(), Some("UA"), "never the one that failed");
    assert!(store.snapshot(&ids[..1]).unwrap().is_some(), "nothing half removed");
    // Set up again later (found damaged): still the first sign-in's.
    store.rebuild().unwrap();
    assert_eq!(store.meta(&format!("{OWNER}{a}")).unwrap().as_deref(), Some("UA"));
}
