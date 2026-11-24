use super::*;
use std::sync::atomic::AtomicU64;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-picture-cache-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

const AVATAR: &str = concat!("https://avatars.slack", "-edge.com/T1/U1-secretname_72.png");
const EMOJI: &str = concat!("https://emoji.slack", "-edge.com/T1/partyparrot/abc123.png");
const THUMB: &str = concat!("https://files.slack", ".com/files-tmb/T1-F1/report_360.png");

fn png(len: usize) -> Vec<u8> {
    let mut b = PNG.to_vec();
    b.resize(len.max(PNG.len()), 7);
    b
}

/// A clock at `secs` after the epoch that the test moves on.
fn clock(secs: u64) -> (Clock, Arc<AtomicU64>) {
    let at = Arc::new(AtomicU64::new(secs));
    let read = at.clone();
    (Arc::new(move || UNIX_EPOCH + Duration::from_secs(read.load(Ordering::Relaxed))), at)
}

const DAY: u64 = 24 * 3600;
const START: u64 = 1_800_000_000;

/// A cache opened as a run's start opens it.
fn cache(name: &str) -> (PictureCache, Arc<AtomicU64>) {
    let (now, at) = clock(START);
    let c = PictureCache::new(scratch(name)).with_clock(now);
    assert_eq!(c.open(), Some(true), "made now");
    (c, at)
}

#[test]
fn a_picture_kept_is_read_back_and_a_miss_is_a_miss() {
    let (c, _) = cache("roundtrip");
    let photo = Source::Avatar(AVATAR.into());
    assert_eq!(c.load(&photo, "photo-4x2"), Loaded::Miss, "no folder yet");
    assert_eq!(c.store(&photo, "photo-4x2", &png(100)), Stored::Stored(None));
    assert_eq!(c.load(&photo, "photo-4x2"), Loaded::Hit(png(100)));
    assert_eq!(c.load(&photo, "chip-2x1"), Loaded::Miss, "another shape is another picture");
    assert_eq!(c.load(&Source::Emoji(AVATAR.into()), "photo-4x2"), Loaded::Miss, "another kind too");
    let _ = fs::remove_dir_all(c.dir());
}

#[test]
fn the_key_is_stable_and_no_file_name_tells_an_address() {
    // SHA-256's published value of no bytes, and one name worked out apart from slakio: the
    // key never changes with the Rust version or the machine.
    let hex = |b: &[u8]| b.iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(hex(&sha256(&[])), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    let photo = Source::Avatar(AVATAR.into());
    assert_eq!(file_name(&photo, "photo-4x2"), "82fe2dccd2a9beb20a9424ea852c904e.png");
    assert_eq!(hex(&key(&photo, "photo-4x2")), "82fe2dccd2a9beb20a9424ea852c904eeb0b2fbe97dd3e93f3ccf2e5bd7921af");
    let (c, _) = cache("names");
    for s in [Source::Avatar(AVATAR.into()), Source::Emoji(EMOJI.into()), Source::Thumb(THUMB.into())] {
        assert!(matches!(c.store(&s, "preview", &png(64)), Stored::Stored(_)));
    }
    let names: Vec<String> =
        fs::read_dir(c.dir()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names.len(), 3, "{names:?}");
    for name in &names {
        assert!(is_name(name), "{name}");
        for part in ["slack", "secretname", "partyparrot", "report", "T1", "U1", "F1", "abc123", "http"] {
            assert!(!name.contains(part), "{name} tells {part}");
        }
        let bytes = fs::read(c.dir().join(name)).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("slack") && !text.contains("secretname") && !text.contains("partyparrot"));
    }
    let _ = fs::remove_dir_all(c.dir());
}

#[cfg(unix)]
#[test]
fn the_folder_is_private_and_so_is_each_file() {
    use std::os::unix::fs::PermissionsExt;
    let (c, _) = cache("perms");
    let photo = Source::Avatar(AVATAR.into());
    c.store(&photo, "photo-4x2", &png(64));
    assert_eq!(fs::metadata(c.dir()).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(fs::metadata(c.path(&photo, "photo-4x2")).unwrap().permissions().mode() & 0o777, 0o600);
    let _ = fs::remove_dir_all(c.dir());
}

#[test]
fn each_kind_expires_after_its_age_and_is_removed() {
    let (c, at) = cache("expiry");
    let (photo, emoji, thumb) =
        (Source::Avatar(AVATAR.into()), Source::Emoji(EMOJI.into()), Source::Thumb(THUMB.into()));
    for s in [&photo, &emoji, &thumb] {
        c.store(s, "x", &png(64));
    }
    at.store(START + 7 * DAY - 60, Ordering::Relaxed);
    for s in [&photo, &emoji, &thumb] {
        assert!(matches!(c.load(s, "x"), Loaded::Hit(_)), "{s:?} still fresh");
    }
    at.store(START + 7 * DAY + 60, Ordering::Relaxed);
    assert_eq!(c.load(&photo, "x"), Loaded::Expired);
    assert_eq!(c.load(&thumb, "x"), Loaded::Expired);
    assert!(!c.path(&photo, "x").exists() && !c.path(&thumb, "x").exists(), "removed");
    assert!(matches!(c.load(&emoji, "x"), Loaded::Hit(_)), "emoji keep 30 days");
    at.store(START + 30 * DAY + 60, Ordering::Relaxed);
    assert_eq!(c.load(&emoji, "x"), Loaded::Expired);
    assert_eq!(c.load(&emoji, "x"), Loaded::Miss);
    let _ = fs::remove_dir_all(c.dir());
}

#[test]
fn a_file_written_in_the_future_is_refused() {
    let (c, at) = cache("future");
    let photo = Source::Avatar(AVATAR.into());
    at.store(START + 3 * DAY, Ordering::Relaxed);
    c.store(&photo, "x", &png(64));
    at.store(START, Ordering::Relaxed);
    assert_eq!(c.load(&photo, "x"), Loaded::Refused("future"));
    assert!(!c.path(&photo, "x").exists());
    let _ = fs::remove_dir_all(c.dir());
}

#[test]
fn the_sweep_keeps_the_folder_within_its_room_least_recently_used_first() {
    let (now, at) = clock(START);
    let c = PictureCache::new(scratch("sweep")).with_clock(now).with_budget(10_000);
    c.open().unwrap();
    let sources: Vec<Source> = (0..8).map(|n| Source::Emoji(format!("{EMOJI}?n={n}"))).collect();
    for (n, s) in sources.iter().enumerate() {
        at.store(START + n as u64 * 10, Ordering::Relaxed);
        let stored = c.store(s, "emoji", &png(2_000));
        let total: u64 = fs::read_dir(c.dir()).unwrap().flatten().map(|e| e.metadata().unwrap().len()).sum();
        assert!(total <= 10_000, "after {n}: {total} bytes ({stored:?})");
    }
    // The first ones went; a read counts as a use.
    assert_eq!(c.load(&sources[0], "emoji"), Loaded::Miss);
    assert!(matches!(c.load(&sources[4], "emoji"), Loaded::Hit(_)));
    at.store(START + 1000, Ordering::Relaxed);
    assert!(matches!(c.load(&sources[4], "emoji"), Loaded::Hit(_)));
    at.store(START + 1010, Ordering::Relaxed);
    c.store(&Source::Emoji(format!("{EMOJI}?n=new")), "emoji", &png(2_000));
    assert!(matches!(c.load(&sources[4], "emoji"), Loaded::Hit(_)), "just used: kept");
    assert_eq!(c.load(&sources[5], "emoji"), Loaded::Miss, "the least recently used went");
    let swept = c.sweep().unwrap();
    // Swept down to four fifths of the room when the new one took it over.
    assert!(swept.bytes <= 8_000 && swept.kept == 3, "{swept:?}");
    let _ = fs::remove_dir_all(c.dir());
}

#[test]
fn the_sweep_removes_what_is_too_old_and_what_this_never_wrote() {
    let (c, at) = cache("sweep-old");
    let photo = Source::Avatar(AVATAR.into());
    let emoji = Source::Emoji(EMOJI.into());
    c.store(&photo, "x", &png(64));
    c.store(&emoji, "x", &png(64));
    fs::write(c.dir().join("notes.txt"), b"planted").unwrap();
    fs::write(c.dir().join("0123456789abcdef.png"), b"not a header").unwrap();
    fs::create_dir(c.dir().join("keep-me")).unwrap();
    at.store(START + 8 * DAY, Ordering::Relaxed);
    let swept = c.sweep().unwrap();
    assert_eq!(swept, Swept { removed: 3, kept: 1, bytes: fs::metadata(c.path(&emoji, "x")).unwrap().len() });
    assert!(!c.path(&photo, "x").exists());
    assert!(c.dir().join("keep-me").is_dir(), "a folder it never made is left alone");
    let _ = fs::remove_dir_all(c.dir());
}

#[test]
fn a_file_too_large_corrupted_or_of_another_key_is_refused_and_removed() {
    let (c, _) = cache("refused");
    let photo = Source::Avatar(AVATAR.into());
    let other = Source::Avatar(format!("{AVATAR}?other"));
    assert_eq!(c.store(&photo, "x", &png(FILE_MAX as usize)), Stored::Refused("too-large"));
    assert_eq!(c.store(&photo, "x", b"GIF89a not a png"), Stored::Refused("not-png"));
    c.store(&photo, "x", &png(64));
    let path = c.path(&photo, "x");
    for (bytes, why) in [
        (b"garbage".to_vec(), "header"),
        (
            {
                let mut b = fs::read(&path).unwrap();
                b.truncate(HEADER);
                b.extend_from_slice(b"no png here");
                b
            },
            "not-png",
        ),
        (vec![0; FILE_MAX as usize + 1], "too-large"),
    ] {
        c.store(&photo, "x", &png(64));
        fs::write(&path, bytes).unwrap();
        assert_eq!(c.load(&photo, "x"), Loaded::Refused(why));
        assert!(!path.exists(), "{why}: removed");
    }
    // Another picture's file put under this one's name.
    c.store(&other, "x", &png(64));
    fs::rename(c.path(&other, "x"), &path).unwrap();
    assert_eq!(c.load(&photo, "x"), Loaded::Refused("key"));
    assert!(!path.exists());
    let _ = fs::remove_dir_all(c.dir());
}

#[cfg(unix)]
#[test]
fn a_link_or_anything_but_a_file_is_never_read_and_is_removed() {
    let (c, _) = cache("links");
    let photo = Source::Avatar(AVATAR.into());
    c.store(&photo, "x", &png(64));
    let path = c.path(&photo, "x");
    let target = c.dir().with_extension("target");
    fs::write(&target, fs::read(&path).unwrap()).unwrap();
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert_eq!(c.load(&photo, "x"), Loaded::Refused("link"));
    assert!(fs::symlink_metadata(&path).is_err(), "the link is removed");
    assert!(target.exists(), "what it pointed to is not touched");
    fs::create_dir(&path).unwrap();
    assert_eq!(c.load(&photo, "x"), Loaded::Refused("not-file"));
    assert!(!path.exists());
    // A link in place of a file is replaced, never written through.
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let before = fs::read(&target).unwrap();
    assert!(matches!(c.store(&photo, "x", &png(80)), Stored::Stored(_)));
    assert_eq!(fs::read(&target).unwrap(), before);
    let _ = fs::remove_file(&target);
    let _ = fs::remove_dir_all(c.dir());
}

#[cfg(unix)]
#[test]
fn a_folder_that_is_a_link_is_neither_read_nor_written() {
    let (now, _) = clock(START);
    let real = scratch("real-folder");
    fs::create_dir_all(&real).unwrap();
    let link = scratch("linked-folder");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let c = PictureCache::new(link.clone()).with_clock(now);
    assert_eq!(c.open(), None);
    let photo = Source::Avatar(AVATAR.into());
    assert_eq!(c.store(&photo, "x", &png(64)), Stored::Refused("folder"));
    assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
    assert_eq!(c.load(&photo, "x"), Loaded::Miss);
    assert_eq!(c.sweep(), None);
    c.clear().unwrap();
    assert!(fs::symlink_metadata(&link).is_err() && real.is_dir(), "clear removes the link only");
    let _ = fs::remove_dir_all(&real);
}

#[test]
fn clear_removes_everything() {
    let (c, _) = cache("clear");
    c.store(&Source::Thumb(THUMB.into()), "preview", &png(64));
    c.clear().unwrap();
    assert!(!c.dir().exists());
    c.clear().unwrap();
}

#[test]
fn the_folder_is_made_once_at_the_start_and_never_again_by_a_write() {
    let (now, _) = clock(START);
    let dir = scratch("made-once");
    let c = PictureCache::new(dir.clone()).with_clock(now);
    let photo = Source::Avatar(AVATAR.into());
    assert_eq!(c.store(&photo, "x", &png(64)), Stored::Refused("folder"), "not opened: nothing written");
    assert!(!dir.exists());
    assert_eq!(c.open(), Some(true));
    assert_eq!(c.open(), Some(false), "there already");
    assert!(matches!(c.clone().store(&photo, "x", &png(64)), Stored::Stored(_)));
    // `slakio logout` removes the folder while this runs: it stays removed.
    PictureCache::new(dir.clone()).clear().unwrap();
    assert_eq!(c.store(&photo, "x", &png(64)), Stored::Refused("folder"));
    assert!(!dir.exists(), "not made again");
    assert_eq!(c.load(&photo, "x"), Loaded::Miss);
}

#[cfg(unix)]
#[test]
fn a_folder_others_may_write_in_is_neither_read_nor_written() {
    use std::os::unix::fs::PermissionsExt;
    let (c, _) = cache("shared");
    let photo = Source::Avatar(AVATAR.into());
    assert!(matches!(c.store(&photo, "x", &png(64)), Stored::Stored(_)));
    fs::set_permissions(c.dir(), fs::Permissions::from_mode(0o777)).unwrap();
    assert_eq!(c.store(&photo, "x", &png(80)), Stored::Refused("folder"));
    assert_eq!(c.load(&photo, "x"), Loaded::Miss, "not read");
    assert_eq!(c.sweep(), None);
    fs::set_permissions(c.dir(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(c.load(&photo, "x"), Loaded::Hit(b) if b == png(64)), "the file left as it was");
    let _ = fs::remove_dir_all(c.dir());
}

#[test]
fn a_file_of_the_first_format_is_refused_and_removed() {
    let (c, _) = cache("v1");
    let photo = Source::Avatar(AVATAR.into());
    // The first format: its magic, an FNV key of 8 bytes, the time, the kind, the PNG.
    let mut v1 = b"slkpic1\n".to_vec();
    v1.extend_from_slice(&0x0123_4567_89ab_cdef_u64.to_le_bytes());
    v1.extend_from_slice(&START.to_le_bytes());
    v1.push(1);
    v1.extend_from_slice(&png(64));
    // Under this format's name: refused on read, also when all but its magic reads as this
    // format.
    fs::write(c.path(&photo, "x"), &v1).unwrap();
    assert_eq!(c.load(&photo, "x"), Loaded::Refused("header"));
    assert!(!c.path(&photo, "x").exists(), "removed");
    assert!(matches!(c.store(&photo, "x", &png(64)), Stored::Stored(_)));
    let mut old_magic = fs::read(c.path(&photo, "x")).unwrap();
    old_magic[..8].copy_from_slice(b"slkpic1\n");
    fs::write(c.path(&photo, "x"), &old_magic).unwrap();
    assert_eq!(c.load(&photo, "x"), Loaded::Refused("header"));
    assert!(!c.path(&photo, "x").exists(), "removed");
    // Under the first format's name (16 hex digits): the sweep removes it.
    fs::write(c.dir().join("0123456789abcdef.png"), &v1).unwrap();
    assert!(matches!(c.store(&photo, "x", &png(64)), Stored::Stored(_)));
    assert_eq!(c.sweep().unwrap(), Swept { removed: 1, kept: 1, bytes: (HEADER + 64) as u64 });
    assert!(!c.dir().join("0123456789abcdef.png").exists());
    let _ = fs::remove_dir_all(c.dir());
}

#[test]
fn a_full_folder_is_swept_to_a_low_mark_so_writes_seldom_sweep() {
    let (now, at) = clock(START);
    let c = PictureCache::new(scratch("low-mark")).with_clock(now).with_budget(100_000);
    c.open().unwrap();
    let mut sweeps = 0;
    for n in 0..100 {
        at.store(START + n, Ordering::Relaxed);
        let s = Source::Emoji(format!("{EMOJI}?n={n}"));
        match c.store(&s, "emoji", &png(2_000)) {
            Stored::Stored(Some(swept)) => {
                sweeps += 1;
                assert!(swept.bytes <= 80_000, "{swept:?}");
            }
            Stored::Stored(None) => {}
            other => panic!("{other:?}"),
        }
    }
    // About 49 fit; each sweep frees room for about 10 more.
    assert!((1..=6).contains(&sweeps), "{sweeps} sweeps for 50 writes past the room");
    let _ = fs::remove_dir_all(c.dir());
}

#[test]
fn a_picture_written_again_counts_once() {
    let (now, _) = clock(START);
    let c = PictureCache::new(scratch("rewrite")).with_clock(now).with_budget(10_000);
    c.open().unwrap();
    let photo = Source::Avatar(AVATAR.into());
    for _ in 0..50 {
        assert_eq!(c.store(&photo, "x", &png(2_000)), Stored::Stored(None), "one file, well within the room");
    }
    assert_eq!(c.load(&photo, "x"), Loaded::Hit(png(2_000)));
    let _ = fs::remove_dir_all(c.dir());
}

#[test]
fn a_sweep_that_panicked_leaves_the_next_one_to_run() {
    let (c, _) = cache("poisoned");
    let photo = Source::Avatar(AVATAR.into());
    assert!(matches!(c.store(&photo, "x", &png(64)), Stored::Stored(_)));
    let held = c.clone();
    let panicked = std::thread::spawn(move || {
        let _one = held.sweeping.lock().unwrap();
        panic!("a sweep panicked part way");
    })
    .join();
    assert!(panicked.is_err() && c.sweeping.is_poisoned());
    let swept = c.sweep().expect("the next sweep runs");
    assert_eq!((swept.removed, swept.kept), (0, 1));
    // Another sweep running meanwhile: this one does not wait for it.
    let running = c.sweeping.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(c.sweep(), None);
    drop(running);
    let _ = fs::remove_dir_all(c.dir());
}

#[test]
fn emptied_every_file_goes_and_the_folder_stays_for_what_comes_next() {
    let (c, _) = cache("emptied");
    let (photo, emoji) = (Source::Avatar(AVATAR.into()), Source::Emoji(EMOJI.into()));
    c.store(&photo, "x", &png(64));
    c.store(&emoji, "x", &png(64));
    c.empty();
    assert_eq!(fs::read_dir(c.dir()).unwrap().count(), 0);
    assert_eq!(c.load(&photo, "x"), Loaded::Miss);
    assert!(matches!(c.store(&photo, "x", &png(64)), Stored::Stored(None)), "kept again from now on");
    assert_eq!(fs::read_dir(c.dir()).unwrap().count(), 1);
    let _ = fs::remove_dir_all(c.dir());
}

#[test]
fn a_write_asked_before_the_folder_was_emptied_is_refused() {
    let (c, _) = cache("epoch");
    let before = c.epoch();
    c.empty();
    let src = Source::Avatar("https://x.test/a.png".into());
    assert_eq!(c.store_as_of(before, &src, "photo", &png(10)), Stored::Refused("stale"));
    assert_eq!(c.load(&src, "photo"), Loaded::Miss);
    assert!(matches!(c.store_as_of(c.epoch(), &src, "photo", &png(10)), Stored::Stored(_)));
    let _ = std::fs::remove_dir_all(c.dir());
}
