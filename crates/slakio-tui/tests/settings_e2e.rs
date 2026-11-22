//! The language row of the settings screen across a restart of the real binary (`--demo`,
//! network off) in a pseudo terminal: set to `ko`, it is saved as `language = "ko"` (the
//! language stays until then), and the next start draws in Korean (its English words gone).
//! Unix only.

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

#[test]
fn the_language_set_on_the_settings_screen_is_used_at_the_next_start() {
    let dir = std::env::temp_dir().join(format!("slakio-settings-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let scratch = Scratch(dir);
    let file = scratch.0.join("config.toml");
    std::fs::write(&file, "# mine\nicons = \"off\"\ncache = \"off\"\n").unwrap();
    let home = scratch.0.join("home");
    let home = home.to_str().unwrap();
    let env = [("SLAKIO_NETWORK", "off"), ("HOME", home)];
    let out = scratch.0.join("typescript-1");
    let Some(mut run) = Run::spawn_with(&scratch.0, &file, &out, &env, (120, 40), "--demo") else { return };
    run.wait_for("the demo", |s| s.contains("backend"));
    run.type_bytes(b" ,");
    run.wait_for("the settings screen", |s| s.contains("Settings") && s.contains("Language"));
    // The language row is the seventh: down six, then the value after `auto` twice.
    run.type_bytes(b"jjjjjj");
    run.type_bytes(b"ll");
    run.wait_for("ko, for the next launch", |s| s.contains("‹ ko") && s.contains("applies on next launch"));
    assert!(run.output().contains("j/k move"), "the language stays until the next launch");
    run.type_bytes(b"\x1b");
    run.wait_for("the screen closed", |s| !s.contains("j/k move"));
    run.quit();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("# mine\n") && text.contains("language = \"ko\""), "{text}");
    let out = scratch.0.join("typescript-2");
    let Some(mut run) = Run::spawn_with(&scratch.0, &file, &out, &env, (120, 40), "--demo") else { return };
    run.wait_for("the demo", |s| s.contains("backend") || s.contains("demo"));
    run.type_bytes(b" ,");
    // Hangul is drawn two cells wide (the small emulator spaces it): the English words are gone.
    run.wait_for("the settings screen", |s| s.contains("‹ ko"));
    let screen = run.output();
    assert!(!screen.contains("Settings") && !screen.contains("j/k move"), "in Korean:\n{screen}");
    run.type_bytes(b"\x1b");
    run.wait_for("the screen closed", |s| !s.contains("‹ ko"));
    run.quit();
}
