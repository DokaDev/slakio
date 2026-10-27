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
