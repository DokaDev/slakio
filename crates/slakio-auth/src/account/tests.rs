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

#[test]
fn signing_in_to_another_workspace_keeps_the_first_and_signing_out_of_one_keeps_the_other() {
    let (store, path) = (MemoryStore::new(), scratch("switch"));
    let creds = Credentials::new(TOKEN, COOKIE);
    keep(&store, &path, &account(), &creds).unwrap();
    let other = Account { team: "T1OTHER".into(), name: "Other".into(), host: "other.slack.com".into(), ..account() };
    keep(&store, &path, &other, &Credentials::new(TOKEN, "other-cookie")).unwrap();
    assert!(store.get("T0TEST").unwrap().is_some(), "the first workspace's secret stays");
    assert!(store.get("T1OTHER").unwrap().is_some());
    assert_eq!(load(&path).unwrap(), vec![account(), other.clone()], "in the order signed in to");
    // Signing in again to the first keeps its place and takes its new name and secret.
    let renamed = Account { name: "Renamed".into(), ..account() };
    keep(&store, &path, &renamed, &Credentials::new(TOKEN, "new-cookie")).unwrap();
    assert_eq!(load(&path).unwrap(), vec![renamed.clone(), other.clone()]);
    assert_eq!(credentials(&store, &renamed).unwrap().unwrap().cookie(), "new-cookie");
    assert_eq!(credentials(&store, &other).unwrap().unwrap().cookie(), "other-cookie", "not touched");
    // Signing out of one: the other stays signed in.
    assert_eq!(forget_one(&store, &path, "T0TEST").unwrap(), Some(renamed));
    assert_eq!(store.get("T0TEST"), Ok(None));
    assert_eq!(load(&path).unwrap(), vec![other.clone()]);
    assert_eq!(forget_one(&store, &path, "T0TEST").unwrap(), None, "not signed in: nothing to do");
    // The last one: the file goes too.
    assert_eq!(forget_one(&store, &path, "T1OTHER").unwrap(), Some(other));
    assert!(!path.exists());
    assert_eq!(forget(&store, &path).unwrap(), vec![]);
}

#[test]
fn signing_out_of_every_workspace_removes_every_secret() {
    let (store, path) = (MemoryStore::new(), scratch("forget-all"));
    let creds = Credentials::new(TOKEN, COOKIE);
    let other = Account { team: "T1OTHER".into(), ..account() };
    keep(&store, &path, &account(), &creds).unwrap();
    keep(&store, &path, &other, &creds).unwrap();
    assert_eq!(forget(&store, &path).unwrap(), vec![account(), other]);
    assert_eq!((store.get("T0TEST"), store.get("T1OTHER")), (Ok(None), Ok(None)));
}

#[test]
fn a_workspace_is_named_by_its_team_name_or_host() {
    let a = Account { team: "T1".into(), name: "Acme".into(), host: "acme.slack.com".into(), user: "U1".into() };
    let b = Account { team: "T2".into(), name: "acme".into(), host: "acme-eu.slack.com".into(), user: "U2".into() };
    let all = [a.clone(), b.clone()];
    assert_eq!(named(&all, "T2"), vec![&b]);
    assert_eq!(named(&all, "acme-eu"), vec![&b]);
    assert_eq!(named(&all, "ACME.slack.com"), vec![&a]);
    assert_eq!(named(&all, "Acme"), vec![&a, &b], "two of that name: both, to be told apart");
    assert!(named(&all, "nothing").is_empty());
}

#[test]
fn an_unreadable_file_is_moved_aside_and_signing_out_of_all_still_finds_its_workspaces() {
    let (store, path) = (MemoryStore::new(), scratch("aside"));
    let creds = Credentials::new(TOKEN, COOKIE);
    let other = Account { team: "T1OTHER".into(), ..account() };
    keep(&store, &path, &account(), &creds).unwrap();
    keep(&store, &path, &other, &creds).unwrap();
    // Damaged: the second workspace lost its host, so nothing of the file can be read.
    let text =
        std::fs::read_to_string(&path).unwrap().replace("host = \"example.slack.com\"\nuser = \"U0TEST\"\n\n", "");
    std::fs::write(&path, &text).unwrap();
    assert!(load(&path).is_err());
    let third = Account { team: "T2THIRD".into(), ..account() };
    keep(&store, &path, &third, &creds).unwrap();
    assert_eq!(load(&path).unwrap(), vec![third.clone()]);
    let kept = std::fs::read_to_string(aside(&path)).unwrap();
    assert!(kept.contains("T0TEST") && kept.contains("T1OTHER"), "not written over: {kept}");
    // An entry no file names (left by an earlier failure) goes too.
    store.set("T3ORPHAN", "v1\na\nb\n").unwrap();
    let gone: Vec<String> = forget(&store, &path).unwrap().into_iter().map(|a| a.team).collect();
    assert_eq!(gone, ["T2THIRD", "T0TEST", "T1OTHER", "T3ORPHAN"]);
    assert_eq!(store.accounts().unwrap(), Vec::<String>::new(), "no secret is left");
    assert!(!path.exists() && !aside(&path).exists());
    // Unreadable and not moved aside yet: its team ids are found all the same.
    keep(&store, &path, &account(), &creds).unwrap();
    std::fs::write(&path, "[[workspace]]\nteam = \"T0TEST\"\n").unwrap();
    assert_eq!(forget(&store, &path).unwrap().len(), 1);
    assert_eq!(store.get("T0TEST"), Ok(None));
}

#[test]
fn signing_in_or_out_waits_for_another_that_changes_the_file() {
    let (store, path) = (std::sync::Arc::new(MemoryStore::new()), scratch("lock"));
    keep(&*store, &path, &account(), &Credentials::new(TOKEN, COOKIE)).unwrap();
    let other = Account { team: "T1OTHER".into(), ..account() };
    let held = std::fs::OpenOptions::new().write(true).open(path.with_extension("lock")).unwrap();
    held.lock().unwrap();
    let (s, p, o) = (std::sync::Arc::clone(&store), path.clone(), other.clone());
    let signing_in = std::thread::spawn(move || keep(&*s, &p, &o, &Credentials::new(TOKEN, COOKIE)));
    let (s, p) = (std::sync::Arc::clone(&store), path.clone());
    let signing_out = std::thread::spawn(move || forget_one(&*s, &p, "T0TEST"));
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(!signing_in.is_finished() && !signing_out.is_finished(), "both wait for the lock");
    assert_eq!(load(&path).unwrap(), vec![account()], "nothing changed meanwhile");
    drop(held);
    signing_in.join().unwrap().unwrap();
    signing_out.join().unwrap().unwrap();
    assert_eq!(load(&path).unwrap(), vec![other], "neither change is lost");
}

#[test]
fn a_team_named_twice_counts_once() {
    let path = scratch("twice");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let line =
        |name: &str| format!("[[workspace]]\nteam = \"T0TEST\"\nname = \"{name}\"\nhost = \"h\"\nuser = \"u\"\n");
    std::fs::write(&path, format!("{}{}", line("first"), line("second"))).unwrap();
    let all = load(&path).unwrap();
    assert_eq!(all.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["first"]);
}

#[cfg(unix)]
#[test]
fn the_workspaces_files_are_readable_by_this_user_alone() {
    use std::os::unix::fs::PermissionsExt;
    let (store, path) = (MemoryStore::new(), scratch("private"));
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    keep(&store, &path, &account(), &Credentials::new(TOKEN, COOKIE)).unwrap();
    assert_eq!(mode(&path), 0o600, "workspaces.toml");
    assert_eq!(mode(&path.with_extension("lock")), 0o600, "workspaces.lock");
    // One that cannot be read is moved aside, and the new one written: both private.
    std::fs::write(&path, "[[workspace]]\nteam = \"T0TEST\"\n").unwrap();
    keep(&store, &path, &account(), &Credentials::new(TOKEN, COOKIE)).unwrap();
    assert_eq!(mode(&aside(&path)), 0o600, "workspaces.toml.unreadable");
    assert_eq!(mode(&path), 0o600);
}
