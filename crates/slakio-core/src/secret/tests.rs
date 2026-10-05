use super::*;

#[test]
fn the_store_switch_reads_memory_and_keychain_and_rejects_anything_else() {
    assert_eq!(StoreKind::from_env(None), Ok(StoreKind::Keychain));
    assert_eq!(StoreKind::from_env(Some("")), Ok(StoreKind::Keychain));
    assert_eq!(StoreKind::from_env(Some(" Memory ")), Ok(StoreKind::Memory));
    assert_eq!(StoreKind::from_env(Some("KEYCHAIN")), Ok(StoreKind::Keychain));
    // A typo must never fall through to the real keychain.
    let e = StoreKind::from_env(Some("memroy")).unwrap_err();
    assert!(e.contains("SLAKIO_SECRET_STORE") && e.contains("memroy"), "{e}");
    assert_eq!(StoreKind::Memory.as_str(), "memory");
}

#[test]
fn the_memory_store_keeps_secrets_per_account() {
    let s = MemoryStore::new();
    assert_eq!(s.get("T1"), Ok(None));
    s.set("T1", "one").unwrap();
    s.set("T2", "two").unwrap();
    assert_eq!(s.get("T1"), Ok(Some("one".into())));
    assert_eq!(s.delete("T1"), Ok(true));
    assert_eq!(s.delete("T1"), Ok(false), "deleting a missing entry is not an error");
    assert_eq!(s.get("T2"), Ok(Some("two".into())));
}

#[test]
fn an_unusable_store_fails_every_call_instead_of_answering_nothing_stored() {
    let s = MemoryStore::unavailable("locked");
    let Err(Unavailable(f)) = s.get("T1") else { panic!("must fail") };
    assert_eq!((f.kind, f.detail.as_str()), (crate::fault::FaultKind::SecretStore, "locked"));
    assert!(s.set("T1", "x").is_err());
    assert!(s.delete("T1").is_err());
}
