//! Splits in the real binary (`--demo`, network off) in a pseudo terminal: a conversation
//! opened, `v` and `s` split it, `Ctrl+H` moves back, `z` zooms and `Ctrl+W` closes, at 120x40
//! and 80x24. Unix only.

#![cfg(unix)]

#[path = "support/pty.rs"]
mod pty;

use pty::Run;
use std::path::PathBuf;

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

#[test]
fn the_demo_splits_moves_zooms_and_closes_panes() {
    for (w, h) in [(120u16, 40u16), (80, 24)] {
        let dir = std::env::temp_dir().join(format!("slakio-split-{}-{w}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let scratch = Scratch(dir);
        let config = scratch.0.join("config.toml");
        std::fs::write(&config, "icons = \"off\"\ncache = \"off\"\n").unwrap();
        let out = scratch.0.join("typescript");
        let env = [("SLAKIO_NETWORK", "off")];
        let Some(mut run) = Run::spawn_with(&scratch.0, &config, &out, &env, (w, h), "--demo") else { return };
        run.wait_for("the demo", |s| s.contains("backend"));
        // A conversation of the list opens.
        run.type_bytes(b"j\r");
        run.wait_for("a conversation open", |s| composers(s) == 1);
        run.type_bytes(b"v");
        run.wait_for("two panes side by side", |s| composers(s) == 2);
        run.type_bytes(b"s");
        run.wait_for("three panes", |s| composers(s) == 3);
        // `Ctrl+H` (sent as Backspace by some terminals; `Space w h` everywhere).
        run.type_bytes(b" wh");
        run.type_bytes(b"z");
        run.wait_for("zoomed", |s| s.contains("[zoomed]") && composers(s) == 1);
        run.type_bytes(b"z");
        run.wait_for("every pane again", |s| !s.contains("[zoomed]") && composers(s) == 3);
        run.type_bytes(b"\x17");
        run.wait_for("a pane closed", |s| composers(s) == 2);
        run.quit();
    }
}
