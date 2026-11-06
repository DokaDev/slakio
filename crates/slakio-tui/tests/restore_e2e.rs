//! The layout across a restart of the real binary (`--demo`, network off) in a pseudo
//! terminal: three panes made, resized and left with `:qa`; the next start shows the same three
//! panes at once, from the private layout file in the state directory; a file that cannot be
//! read is said once and the app starts without it; `restore = "off"` opens nothing. At 120x40
//! and 80x24. Unix only.

#![cfg(unix)]

#[path = "support/pty.rs"]
mod pty;

use pty::Run;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A scratch directory of its own, removed when dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn composers(screen: &str) -> usize {
    screen.matches("Message #").count()
}

/// The rows the composers' dividers are on, as drawn, and how many on each.
fn rows(s: &str) -> Vec<(usize, usize)> {
    s.lines().enumerate().map(|(y, l)| (y, l.matches("Message #").count())).filter(|(_, n)| *n > 0).collect()
}

fn start(dir: &Path, config: &str, size: (u16, u16), run: usize) -> Option<Run> {
    let file = dir.join("config.toml");
    std::fs::write(&file, config).unwrap();
    let out = dir.join(format!("typescript-{run}"));
    Run::spawn_with(dir, &file, &out, &[("SLAKIO_NETWORK", "off")], size, "--demo")
}

#[test]
fn three_panes_come_back_after_a_restart() {
    for (w, h) in [(120u16, 40u16), (80, 24)] {
        let dir = std::env::temp_dir().join(format!("slakio-restore-{}-{w}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let scratch = Scratch(dir);
        let config = "icons = \"off\"\ncache = \"off\"\n";
        let Some(mut run) = start(&scratch.0, config, (w, h), 1) else { return };
        run.wait_for("the demo", |s| s.contains("backend"));
        run.type_bytes(b"j\r");
        run.wait_for("a conversation open", |s| composers(s) == 1);
        run.type_bytes(b"v");
        run.wait_for("two panes", |s| composers(s) == 2);
        run.type_bytes(b"s");
        run.wait_for("three panes", |s| composers(s) == 3);
        let three = rows(&run.output());
        run.type_bytes(b" w-");
        run.wait_for("the bottom pane shorter", |s| rows(s) != three);
        let before = run.output();
        run.quit();
        let file = scratch.0.join("state").join("slakio").join("layout-demo.toml");
        let text = std::fs::read_to_string(&file).expect("the layout is saved");
        assert!(text.contains("version = 1") && text.matches("[[tab.pane]]").count() == 3, "{text}");
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600, "private");
        // The next start: the same three panes, the same sizes, nothing typed.
        let Some(run) = start(&scratch.0, config, (w, h), 2) else { return };
        run.wait_for("the three panes again", |s| composers(s) == 3);
        let after = run.output();
        assert_eq!(rows(&after), rows(&before), "{w}x{h}: the composers where they were");
        run.quit();
        // A file that cannot be read: said, and the app starts without it.
        std::fs::write(&file, "version = 1\n[[tab]]\ntree = [\"pane 9\"]\n").unwrap();
        let Some(run) = start(&scratch.0, config, (w, h), 3) else { return };
        run.wait_for("the notice", |s| s.contains("Saved layout unreadable"));
        assert_eq!(composers(&run.output()), 0);
        run.quit();
        // Turned off: nothing opened, nothing written.
        std::fs::write(&file, &text).unwrap();
        let Some(run) = start(&scratch.0, &format!("{config}restore = \"off\"\n"), (w, h), 4) else { return };
        run.wait_for("the demo", |s| s.contains("backend"));
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert_eq!(composers(&run.output()), 0, "{}", run.output());
        run.quit();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text, "left as it was");
    }
}
