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

fn cache(name: &str) -> (PictureCache, Arc<AtomicU64>) {
    let (now, at) = clock(START);
    (PictureCache::new(scratch(name)).with_clock(now), at)
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
    // FNV-1a's published test value: the hash never changes with the Rust version.
    assert_eq!(fnv1a(&[]), 0xcbf2_9ce4_8422_2325);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in b"a" {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    assert_eq!(h, 0xaf63_dc4c_8601_ec8c);
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
    assert!(swept.bytes <= 10_000 && swept.kept == 4, "{swept:?}");
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
