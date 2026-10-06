//! The real binary in a pseudo terminal (`script`): ended by `:qa`, from outside with SIGTERM
//! or SIGHUP, or by a panic, it restores the terminal every time (alternate screen left, mouse
//! and bracketed paste off, the cursor shown with the user's shape). A setting changed while it
//! runs reaches the config file. Unix only.
//!
//! When `script` cannot start, each test prints a visible `SKIPPED` line with the reason and
//! passes; with `SLAKIO_REQUIRE_PTY=1` (CI on Linux and macOS) it fails instead, so a runner
//! without a pseudo terminal can never turn these tests into silent passes.
//!
//! Every path out of a test (an assertion that fails, a timeout) kills and reaps what it
//! started ([`Run`]): `script` puts the binary in raw mode on a pseudo terminal of its own, so
//! neither ends when the test process does.
#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Set to `1`, a pseudo terminal that cannot start fails the test instead of skipping it.
const REQUIRE_PTY: &str = "SLAKIO_REQUIRE_PTY";

/// One run of the binary inside `script`, with its scratch directory. Dropped on every path
/// (also a panic): the binary and `script` are each killed if they still run and waited for,
/// then the directory is removed.
struct Run {
    child: Child,
    dir: PathBuf,
}

impl Drop for Run {
    fn drop(&mut self) {
        if let Some(pid) = app_pid(&self.dir.join("config.toml")) {
            let _ = Command::new("kill").arg("-KILL").arg(pid.to_string()).status();
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        let deadline = Instant::now() + Duration::from_secs(10);
        while app_pid(&self.dir.join("config.toml")).is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Run {
    /// Start the binary in a new scratch directory named after `tag`, with `env` set; `None`
    /// (with a note) when `script` cannot start.
    fn start(tag: &str, env: &[(&str, &str)]) -> Option<Run> {
        Self::with_config(tag, env, "")
    }

    /// [`Run::start`] with `config` as the config file's text.
    fn with_config(tag: &str, env: &[(&str, &str)], config: &str) -> Option<Run> {
        let dir = std::env::temp_dir().join(format!("slakio-signals-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), config).unwrap();
        let child = start(&dir, env)?;
        Some(Run { child, dir })
    }

    fn type_bytes(&mut self, bytes: &[u8]) {
        let stdin = self.child.stdin.as_mut().expect("stdin");
        stdin.write_all(bytes).unwrap();
        stdin.flush().unwrap();
    }

    /// What the binary wrote to its terminal so far.
    fn output(&self) -> String {
        std::fs::read(self.dir.join("typescript")).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
    }

    /// Wait until the first frame is out: the alternate screen is on and a frame went out (it
    /// hides the cursor).
    fn drawn(&self) {
        wait_for("the first frame", 20, || {
            let t = self.output();
            t.find("\x1b[?1049h").is_some_and(|at| t[at..].contains("\x1b[?25l"))
        });
    }

    fn app_pid(&self) -> u32 {
        app_pid(&self.dir.join("config.toml")).expect("the binary's process")
    }

    fn wait_end(&mut self, what: &str) {
        wait_for(what, 20, || self.child.try_wait().ok().flatten().is_some());
    }
}

/// Start the binary inside `script`, recording what it writes to the terminal in
/// `<dir>/typescript`. Every directory it could read or write is inside `dir`, and credentials
/// stay in memory.
fn start(dir: &Path, env: &[(&str, &str)]) -> Option<Child> {
    let bin = env!("CARGO_BIN_EXE_slakio");
    let config = dir.join("config.toml");
    let out = dir.join("typescript");
    let mut cmd = Command::new("script");
    if cfg!(target_os = "linux") {
        let line = format!("{bin} --config {}", config.display());
        cmd.args(["-q", "-f", "-e", "-c", &line]).arg(&out);
    } else {
        cmd.args(["-q", "-F"]).arg(&out).arg(bin).arg("--config").arg(&config);
    }
    cmd.env("SLAKIO_SECRET_STORE", "memory")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("TERM", "xterm-256color")
        .env_remove("TMUX")
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let required = std::env::var(REQUIRE_PTY).is_ok_and(|v| v == "1");
    spawn_or_skip(&mut cmd, required)
}

/// Start `cmd`. When it cannot start: a failure when `required`, else `None` after a visible
/// `SKIPPED` line on the terminal (written past the test harness's capture, so it shows even
/// when the test passes).
fn spawn_or_skip(cmd: &mut Command, required: bool) -> Option<Child> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    match cmd.spawn() {
        Ok(child) => Some(child),
        Err(e) if required => panic!("`{program}` could not start ({e}), and {REQUIRE_PTY}=1 requires it"),
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "SKIPPED signals: `{program}` could not start: {e}");
            None
        }
    }
}

/// The process id of the binary: a process whose command line names `config` (so no other
/// slakio) and whose program is slakio (not `script`, nor a shell it started).
fn app_pid(config: &Path) -> Option<u32> {
    let out = Command::new("pgrep").arg("-f").arg(config.display().to_string()).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.trim().parse::<u32>().ok()).find(|pid| {
        Command::new("ps")
            .args(["-o", "comm=", "-p", &pid.to_string()])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim().ends_with("slakio"))
    })
}

fn wait_for(what: &str, secs: u64, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The terminal was given back after the last time the alternate screen was entered.
fn assert_restored(text: &str, what: &str) {
    let after = &text[text.rfind("\x1b[?1049h").expect("entered the alternate screen")..];
    for (seq, part) in [
        ("\x1b[?2004l", "bracketed paste off"),
        ("\x1b[?1000l", "mouse capture off"),
        ("\x1b[?25h", "the cursor shown"),
        ("\x1b[0 q", "the user's cursor shape"),
        ("\x1b[?1049l", "the main screen"),
    ] {
        assert!(after.contains(seq), "{what}: {part} ({seq:?}) missing from {after:?}");
    }
    // The alternate screen is left last: nothing is drawn after it.
    assert!(after.rfind("\x1b[?1049l") > after.rfind("\x1b[0 q"), "{what}: {after:?}");
}

#[test]
fn colon_qa_quits_and_restores_the_terminal() {
    let Some(mut run) = Run::start("qa", &[]) else { return };
    run.drawn();
    run.type_bytes(b":qa\r");
    run.wait_end("the binary to quit after :qa");
    assert_restored(&run.output(), ":qa");
}

/// `:theme` while running draws with the theme at once and saves it in the config file, which
/// keeps its comments and other settings.
#[test]
fn a_theme_picked_while_running_is_saved_keeping_the_files_comments() {
    let config = "# my settings\nlanguage = \"en\" # English, please\ntheme = \"auto\"\n";
    let Some(mut run) = Run::with_config("theme", &[], config) else { return };
    run.drawn();
    run.type_bytes(b":theme nord\r");
    let path = run.dir.join("config.toml");
    wait_for("the theme to be saved", 20, || std::fs::read_to_string(&path).is_ok_and(|t| t.contains("nord")));
    run.type_bytes(b":qa\r");
    run.wait_end("the binary to quit after :qa");
    let saved = std::fs::read_to_string(&path).unwrap();
    assert_eq!(saved, "# my settings\nlanguage = \"en\" # English, please\ntheme = \"nord\"\n");
}

#[test]
fn sigterm_and_sighup_restore_the_terminal() {
    for signal in ["TERM", "HUP"] {
        let Some(mut run) = Run::start(signal, &[]) else { return };
        run.drawn();
        let killed = Command::new("kill").arg(format!("-{signal}")).arg(run.app_pid().to_string()).status().unwrap();
        assert!(killed.success());
        run.wait_end("the binary to end");
        assert_restored(&run.output(), &format!("SIG{signal}"));
    }
}

/// `SLAKIO_DEBUG_PANIC=1` (debug builds only) panics right after the first frame.
#[test]
fn a_panic_restores_the_terminal_before_its_message() {
    let Some(mut run) = Run::start("panic", &[("SLAKIO_DEBUG_PANIC", "1")]) else { return };
    run.drawn();
    run.wait_end("the binary to end after its panic");
    let text = run.output();
    assert_restored(&text, "panic");
    let left = text.rfind("\x1b[?1049l").unwrap();
    assert!(text[left..].contains("panicked"), "the message comes after the restore: {text:?}");
}

/// A run that fails before it ends the binary leaves no process behind, and its directory is
/// removed.
#[test]
fn a_failed_run_leaves_no_process_behind() {
    let Some(run) = Run::start("failed", &[]) else { return };
    run.drawn();
    let (app, script) = (run.app_pid(), run.child.id());
    let dir = run.dir.clone();
    let alive = |pid: u32| {
        Command::new("kill").arg("-0").arg(pid.to_string()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    };
    assert!(alive(app) && alive(script));
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _run = run;
        panic!("a failed assertion before the binary was ended");
    }));
    assert!(failed.is_err());
    assert!(!alive(app), "the binary still runs");
    assert!(!alive(script), "`script` still runs");
    assert!(!dir.exists(), "{}", dir.display());
}

/// A pseudo terminal that cannot start is skipped visibly, and fails when it is required.
#[test]
fn a_pseudo_terminal_that_cannot_start_fails_when_required() {
    let missing = || {
        let mut cmd = Command::new("slakio-test-no-such-program");
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        cmd
    };
    assert!(spawn_or_skip(&mut missing(), false).is_none(), "skipped when not required");
    let required = std::panic::catch_unwind(|| spawn_or_skip(&mut missing(), true).map(|mut c| c.kill()));
    assert!(required.is_err(), "a required pseudo terminal that cannot start must fail the test");
}
