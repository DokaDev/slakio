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
fn a_set_of_workspaces_has_a_file_of_its_own() {
    let (a, b) = (WorkspaceId::new("T1"), WorkspaceId::new("T2"));
    let ab = file_name(Some(&[a.clone(), b.clone()]));
    assert_eq!(ab, file_name(Some(&[b, a.clone()])), "in any order");
    assert_ne!(ab, file_name(Some(std::slice::from_ref(&a))));
    assert!(ab.starts_with("layout-") && ab.ends_with(".toml") && !ab.contains("T1"), "{ab}");
    assert_eq!(file_name(None), "layout-demo.toml");
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
