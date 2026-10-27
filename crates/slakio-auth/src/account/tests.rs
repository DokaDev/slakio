use super::*;
use slakio_core::secret::MemoryStore;

const TOKEN: &str = concat!("xo", "xc-1-2-TEST");
const COOKIE: &str = concat!("xo", "xd-TEST%2F1");

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-auth-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("workspaces.toml")
}

fn account() -> Account {
    Account {
        team: "T0TEST".into(),
        name: "Example \"team\"".into(),
        host: "example.slack.com".into(),
        user: "U0TEST".into(),
    }
}

#[test]
fn signing_in_keeps_the_secret_in_the_store_and_only_names_in_the_file() {
    let (store, path) = (MemoryStore::new(), scratch("keep"));
    let creds = Credentials::new(TOKEN, COOKIE).with_ds("17");
    keep(&store, &path, &account(), &creds).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains(TOKEN) && !text.contains(COOKIE), "{text}");
    assert_eq!(load(&path).unwrap(), vec![account()]);
    assert_eq!(credentials(&store, &account()).unwrap(), Some(creds));
    assert_eq!(forget(&store, &path).unwrap(), vec![account()]);
    assert!(!path.exists());
    assert_eq!(store.get("T0TEST"), Ok(None));
    assert_eq!(forget(&store, &path).unwrap(), vec![], "signing out twice is not an error");
}

#[test]
fn a_store_that_fails_keeps_nothing() {
    let (store, path) = (MemoryStore::unavailable("locked"), scratch("fail"));
    assert!(keep(&store, &path, &account(), &Credentials::new(TOKEN, COOKIE)).is_err());
    assert!(!path.exists());
}

#[test]
fn a_damaged_file_or_entry_is_a_fault_not_none() {
    let path = scratch("bad");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "[[workspace]]\nteam = \"T1\"\n").unwrap();
    assert!(load(&path).is_err());
    std::fs::write(&path, "[[workspace]]\nteam = \"../x\"\nname = \"n\"\nhost = \"h\"\nuser = \"u\"\n").unwrap();
    assert!(load(&path).is_err(), "a team id names a keychain entry: letters and digits only");
    let store = MemoryStore::new();
    store.set("T0TEST", "garbage").unwrap();
    let e = credentials(&store, &account()).unwrap_err();
    assert!(!e.detail.contains("garbage"));
}

#[test]
fn entries_round_trip() {
    for c in [Credentials::new(TOKEN, COOKIE), Credentials::new(TOKEN, COOKIE).with_ds("1")] {
        assert_eq!(decode(&encode(&c)), Some(c));
    }
    assert_eq!(decode("v2\na\nb\n"), None);
    assert_eq!(decode("v1\n\nb\n"), None);
}
