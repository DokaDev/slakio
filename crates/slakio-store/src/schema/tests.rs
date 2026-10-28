use super::*;

fn memory() -> Connection {
    Connection::open_in_memory().unwrap()
}

fn tables(conn: &Connection) -> Vec<String> {
    let mut st = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").unwrap();
    st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

#[test]
fn a_new_database_gets_every_table_and_the_latest_version() {
    let mut conn = memory();
    assert_eq!(version(&conn).unwrap(), 0);
    assert_eq!(migrate(&mut conn, MIGRATIONS).unwrap(), Schema::Current);
    assert_eq!(version(&conn).unwrap() as usize, MIGRATIONS.len());
    let t = tables(&conn);
    for name in
        ["meta", "workspaces", "users", "sections", "conversations", "saved", "messages", "seen", "schema_version"]
    {
        assert!(t.contains(&name.to_string()), "{name} in {t:?}");
    }
    // Opening again applies nothing twice.
    assert_eq!(migrate(&mut conn, MIGRATIONS).unwrap(), Schema::Current);
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "one row holds the version");
}

#[test]
fn an_older_database_is_brought_up_keeping_what_it_holds() {
    let mut conn = memory();
    migrate(&mut conn, MIGRATIONS).unwrap();
    conn.execute("INSERT INTO meta (key, value) VALUES ('kept', 'yes')", []).unwrap();
    // The next version, as a later release would append it.
    let mut next = MIGRATIONS.to_vec();
    next.push("ALTER TABLE seen ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;");
    assert_eq!(migrate(&mut conn, &next).unwrap(), Schema::Current);
    assert_eq!(version(&conn).unwrap() as usize, next.len());
    let kept: String = conn.query_row("SELECT value FROM meta WHERE key = 'kept'", [], |r| r.get(0)).unwrap();
    assert_eq!(kept, "yes");
    conn.execute("INSERT INTO seen (workspace, conversation, at, pinned) VALUES ('T', 'C', 1, 1)", []).unwrap();
}

#[test]
fn a_migration_that_fails_changes_nothing() {
    let mut conn = memory();
    migrate(&mut conn, MIGRATIONS).unwrap();
    let mut next = MIGRATIONS.to_vec();
    next.push("CREATE TABLE half (x INTEGER); THIS IS NOT SQL;");
    assert!(migrate(&mut conn, &next).is_err());
    assert_eq!(version(&conn).unwrap() as usize, MIGRATIONS.len(), "still the version before");
    assert!(!tables(&conn).contains(&"half".to_string()), "its first statement rolled back too");
}

#[test]
fn a_database_of_a_newer_version_is_left_alone() {
    let mut conn = memory();
    migrate(&mut conn, MIGRATIONS).unwrap();
    conn.execute("UPDATE schema_version SET version = 99", []).unwrap();
    assert_eq!(migrate(&mut conn, MIGRATIONS).unwrap(), Schema::Newer(99));
    assert_eq!(version(&conn).unwrap(), 99, "not changed");
}

#[test]
fn a_file_on_disk_is_brought_up_to_a_later_version_keeping_what_it_holds() {
    use crate::db::{DIR, FILE, Store};
    let named = crate::testutil::scratch("migrate-disk").join("cache");
    // A file as this version leaves it: written through the store, closed.
    let mut store = Store::open(&named).unwrap().store;
    assert_eq!(version(&store.conn).unwrap() as usize, MIGRATIONS.len());
    store.put_snapshot(slakio_world::World::new(3).snapshot()).unwrap();
    store.set_meta("kept", "yes").unwrap();
    drop(store);
    // Opened again by a later version, from the file.
    let path = named.join(DIR).join(FILE);
    let mut next = MIGRATIONS.to_vec();
    next.push("ALTER TABLE seen ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;");
    let mut conn = Connection::open(&path).unwrap();
    assert_eq!(migrate(&mut conn, &next).unwrap(), Schema::Current);
    drop(conn);
    let conn = Connection::open(&path).unwrap();
    assert_eq!(version(&conn).unwrap() as usize, next.len(), "the new version is on disk");
    let kept: String = conn.query_row("SELECT value FROM meta WHERE key = 'kept'", [], |r| r.get(0)).unwrap();
    assert_eq!(kept, "yes");
    let workspaces: i64 = conn.query_row("SELECT COUNT(*) FROM workspaces", [], |r| r.get(0)).unwrap();
    assert!(workspaces > 0, "the snapshot is still there");
    conn.execute("INSERT INTO seen (workspace, conversation, at, pinned) VALUES ('T', 'C', 1, 1)", []).unwrap();
}

#[test]
fn a_version_one_cache_keeps_its_snapshot_per_workspace_and_its_owner() {
    use crate::db::Store;
    let named = crate::testutil::scratch("migrate-v1").join("cache");
    drop(Store::open(&named).unwrap().store);
    let path = named.join(crate::db::DIR).join(crate::db::FILE);
    // A cache as 0.0.7 wrote it: version 1, one snapshot of two workspaces, one owner.
    std::fs::remove_file(&path).unwrap();
    let mut conn = Connection::open(&path).unwrap();
    migrate(&mut conn, &MIGRATIONS[..1]).unwrap();
    conn.execute_batch(
        "INSERT INTO workspaces VALUES (0, 'T1', 'One', 0, 'U1'), (1, 'T2', 'Two', 1, 'U2');
         INSERT INTO sections VALUES (0, 'S1', 'T1', 'Channels', 2), (1, 'S2', 'T2', 'Channels', 2);
         INSERT INTO conversations VALUES (0, 'C1', 'T1', 0, 0, '', 'general', 'S1', 0, 0, 1, 0),
             (1, 'C2', 'T2', 0, 0, '', 'random', 'S2', 0, 0, 0, 0);
         INSERT INTO users VALUES ('T1', 'U1', 0, 'one', 'One', NULL, 0, 3), ('T2', 'U2', 1, 'two', 'Two', NULL, 0, 3);
         INSERT INTO meta VALUES ('snapshot.taken', '42'), ('snapshot.saved', 'unknown'), ('owner', 'T1 U1');",
    )
    .unwrap();
    drop(conn);
    let mut store = Store::open(&named).unwrap().store;
    assert_eq!(version(&store.conn).unwrap() as usize, MIGRATIONS.len());
    assert!(store.claim("T1", "U1").unwrap(), "the owner carried over");
    let one = store.snapshot(&[slakio_core::model::WorkspaceId::new("T1")]).unwrap().expect("T1's part");
    assert_eq!(one.conversations.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["C1"]);
    assert_eq!((one.taken.0, one.saved), (42, None));
    let two = store.snapshot(&[slakio_core::model::WorkspaceId::new("T2")]).unwrap().expect("T2's part");
    assert_eq!(two.users.len(), 1);
    // T2 had no owner of its own: claimed, it is emptied, T1's part stays.
    assert!(store.claim("T2", "U2").unwrap());
    assert_eq!(store.snapshot(&[slakio_core::model::WorkspaceId::new("T2")]).unwrap(), None);
    assert!(store.snapshot(&[slakio_core::model::WorkspaceId::new("T1")]).unwrap().is_some());
}
