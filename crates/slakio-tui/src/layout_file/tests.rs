use super::*;
use slakio_core::layout::saved::{SavedPane, SavedTab};
use slakio_core::layout::{Node, PaneId};
use slakio_core::model::{ConversationId, Target};

/// A scratch directory of its own, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("slakio-layout-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn one(c: &str) -> Saved {
    let target = Target::Conversation { workspace: WorkspaceId::new("T1"), conversation: ConversationId::new(c) };
    let tab = SavedTab {
        name: None,
        root: Node::Leaf(PaneId(0)),
        active: PaneId(0),
        zoomed: false,
        panes: vec![SavedPane { target, panel: None }],
    };
    Saved { tabs: vec![tab], current: 0 }
}

#[test]
fn a_set_of_sign_ins_has_a_file_of_its_own() {
    let a = (WorkspaceId::new("T1"), "U1".to_string());
    let b = (WorkspaceId::new("T2"), "U2".to_string());
    let ab = file_name(Some(&[a.clone(), b.clone()]));
    assert_eq!(ab, file_name(Some(&[b, a.clone()])), "in any order");
    assert_ne!(ab, file_name(Some(std::slice::from_ref(&a))));
    assert!(ab.starts_with("layout-") && ab.ends_with(".toml") && !ab.contains("T1"), "{ab}");
    assert!(!ab.contains("U1"), "{ab}");
    // Another user of the same workspace never reads the first one's tabs.
    let other = (WorkspaceId::new("T1"), "U9".to_string());
    assert_ne!(file_name(Some(&[a])), file_name(Some(&[other])));
    assert_eq!(file_name(None), "layout-demo.toml");
}

#[test]
fn the_layout_is_looked_at_only_when_the_app_says_it_may_have_changed() {
    let s = Scratch::new("stamp");
    let now = Instant::now();
    let mut f = LayoutFile::new(Some(&s.0.join("state")), "layout-demo.toml");
    let looked = std::cell::Cell::new(0);
    let look = || {
        looked.set(looked.get() + 1);
        Some(one("C1"))
    };
    f.keep((1, false), look, now, false).unwrap();
    f.keep((1, false), look, now + EVERY, false).unwrap();
    f.keep((1, false), look, now + EVERY * 2, true).unwrap();
    assert_eq!(looked.get(), 1, "the same stamp: not made or written again");
    f.keep((2, false), look, now + EVERY * 3, false).unwrap();
    f.keep((2, true), look, now + EVERY * 4, false).unwrap();
    assert_eq!(looked.get(), 3);
    // Without a file (`restore = "off"`) the layout is never even made.
    let mut off = LayoutFile::default();
    for n in 0..3 {
        off.keep((n, false), look, now, n == 2).unwrap();
    }
    assert_eq!(looked.get(), 3);
}

#[test]
fn a_layout_that_would_not_read_back_is_never_written() {
    let s = Scratch::new("unreadable-text");
    let mut f = LayoutFile::new(Some(&s.0.join("state")), "layout-demo.toml");
    f.offer(Some(&one("C1")), Instant::now()).unwrap();
    // A tab whose tree names a pane it does not have.
    let mut bad = one("C2");
    bad.tabs[0].root = Node::Leaf(PaneId(1));
    assert!(f.offer(Some(&bad), Instant::now() + EVERY * 2).is_err(), "said");
    assert_eq!(f.deadline(), None, "nothing waits");
    assert!(f.offer(Some(&bad), Instant::now() + EVERY * 3).is_ok(), "said once");
    let mut next = LayoutFile::new(Some(&s.0.join("state")), "layout-demo.toml");
    assert_eq!(next.load().unwrap(), Some(one("C1")), "the file still reads");
    // A good one is written again.
    f.offer(Some(&one("C3")), Instant::now() + EVERY * 4).unwrap();
    assert_eq!(next.load().unwrap(), Some(one("C3")));
}

#[test]
fn a_file_that_could_not_be_read_is_kept_aside_once_before_it_is_written_over() {
    let s = Scratch::new("bak");
    let state = s.0.join("state");
    std::fs::create_dir_all(&state).unwrap();
    let path = state.join("layout-demo.toml");
    let bak = state.join("layout-demo.toml.bak");
    std::fs::write(&bak, "an older one").unwrap();
    std::fs::write(&path, "edited by hand = [").unwrap();
    let mut f = LayoutFile::new(Some(&state), "layout-demo.toml");
    assert!(f.load().is_err());
    f.offer(Some(&one("C1")), Instant::now()).unwrap();
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), "edited by hand = [", "one kept, the newest");
    f.offer(Some(&one("C2")), Instant::now() + EVERY * 2).unwrap();
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), "edited by hand = [", "only the unreadable one");
    assert_eq!(f.load().unwrap(), Some(one("C2")));
    // A file that was read is never kept aside.
    let mut g = LayoutFile::new(Some(&state), "layout-demo.toml");
    g.load().unwrap();
    g.offer(Some(&one("C3")), Instant::now()).unwrap();
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), "edited by hand = [");
}

#[cfg(unix)]
#[test]
fn the_state_directory_is_private_and_a_pipe_is_not_waited_on() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("private");
    let state = s.0.join("state");
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let mut f = LayoutFile::new(Some(&state), "layout-demo.toml");
    f.offer(Some(&one("C1")), Instant::now()).unwrap();
    assert_eq!(mode(&state), 0o700, "made private");
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755)).unwrap();
    f.offer(Some(&one("C2")), Instant::now() + EVERY * 2).unwrap();
    assert_eq!(mode(&state), 0o700, "made private again");
    // A pipe where the file goes: refused at once, not read from (which would wait forever).
    let path = state.join("layout-demo.toml");
    std::fs::remove_file(&path).unwrap();
    let made = std::process::Command::new("mkfifo").arg(&path).status();
    if made.is_ok_and(|s| s.success()) {
        let mut g = LayoutFile::new(Some(&state), "layout-demo.toml");
        assert!(g.load().is_err());
    }
}

#[test]
fn a_layout_is_written_privately_when_it_changes_at_most_once_a_second() {
    let s = Scratch::new("write");
    let now = Instant::now();
    let mut f = LayoutFile::new(Some(&s.0.join("state")), "layout-demo.toml");
    let path = f.path().unwrap().to_path_buf();
    assert_eq!(f.load().unwrap(), None, "no file: no layout");
    f.offer(Some(&one("C1")), now).unwrap();
    assert_eq!(Saved::parse(&std::fs::read_to_string(&path).unwrap()), Ok(one("C1")));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    // A change within the second waits; the deadline writes it.
    f.offer(Some(&one("C2")), now + Duration::from_millis(100)).unwrap();
    assert_eq!(Saved::parse(&std::fs::read_to_string(&path).unwrap()), Ok(one("C1")));
    assert_eq!(f.deadline(), Some(now + EVERY));
    f.flush(now + EVERY).unwrap();
    assert_eq!(Saved::parse(&std::fs::read_to_string(&path).unwrap()), Ok(one("C2")));
    assert_eq!(f.deadline(), None);
    // Nothing changed, nothing waits; no layout to save (no backend) writes nothing.
    f.offer(Some(&one("C2")), now + EVERY * 3).unwrap();
    f.offer(None, now + EVERY * 4).unwrap();
    assert_eq!(f.deadline(), None);
    // Read back by the next run.
    let mut next = LayoutFile::new(Some(&s.0.join("state")), "layout-demo.toml");
    assert_eq!(next.load().unwrap(), Some(one("C2")));
}

#[test]
fn a_file_that_cannot_be_used_is_said_and_never_followed() {
    let s = Scratch::new("bad");
    let state = s.0.join("state");
    std::fs::create_dir_all(&state).unwrap();
    let path = state.join("layout-demo.toml");
    std::fs::write(&path, "version = 1\n[[tab]]\ntree = [\"pane 3\"]\n").unwrap();
    let mut f = LayoutFile::new(Some(&state), "layout-demo.toml");
    assert!(f.load().is_err(), "a broken file");
    std::fs::write(&path, vec![b'#'; MAX_BYTES as usize + 1]).unwrap();
    assert!(f.load().is_err(), "too big");
    // Written over by the next layout.
    f.offer(Some(&one("C1")), Instant::now()).unwrap();
    assert_eq!(f.load().unwrap(), Some(one("C1")));
    #[cfg(unix)]
    {
        let elsewhere = s.0.join("elsewhere");
        std::fs::write(&elsewhere, "keep").unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &path).unwrap();
        let mut f = LayoutFile::new(Some(&state), "layout-demo.toml");
        assert!(f.load().is_err(), "a symbolic link is not read");
        assert!(f.offer(Some(&one("C1")), Instant::now()).is_err(), "nor written through");
        assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "keep");
        // Said once: no write is tried again this run.
        assert!(f.offer(Some(&one("C2")), Instant::now() + EVERY * 2).is_ok());
        assert_eq!(f.deadline(), None);
    }
    // No state directory: nothing read or written.
    let mut none = LayoutFile::new(None, "layout-demo.toml");
    assert_eq!(none.load().unwrap(), None);
    assert!(none.offer(Some(&one("C1")), Instant::now()).is_ok());
}
