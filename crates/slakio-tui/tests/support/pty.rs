//! The real binary in a pseudo terminal (`script`), for the flow tests that run it: the
//! scratch directories it may use, one run and its screen (read through a small emulator of
//! what it writes, since ratatui writes only the cells that changed), and its process. Unix only.
//!
//! When `script` cannot start, a test prints a visible `SKIPPED` line and passes; with
//! `SLAKIO_REQUIRE_PTY=1` (CI on Linux and macOS) it fails instead.

#![allow(dead_code, reason = "each test file that runs the binary uses a part of it")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const REQUIRE_PTY: &str = "SLAKIO_REQUIRE_PTY";
/// Synthetic credentials (spelled in parts so no file holds a token-shaped string).
pub const TOKEN: &str = concat!("xo", "xc-1-2-TESTTOKEN");
pub const COOKIE: &str = concat!("xo", "xd-TESTCOOKIE");

/// The opener every run gets unless a test sets its own: a program that does nothing, so no
/// test can start a real browser.
pub const SAFE_OPENER: &str = if cfg!(windows) { "hostname.exe" } else { "/usr/bin/false" };

/// Every directory the binary could read or write inside `dir`; credentials in memory.
pub fn dirs(cmd: &mut Command, dir: &Path) {
    cmd.env("SLAKIO_SECRET_STORE", "memory")
        .env("SLAKIO_TEST_OPENER", SAFE_OPENER)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        // A warm start draws from the cache however slow the machine (the read is waited for).
        .env("SLAKIO_TEST_CACHE_WAIT_MS", "10000")
        .env("LC_ALL", "en_US.UTF-8")
        .env("TERM", "xterm-256color")
        .env_remove("TMUX")
        .env_remove("SLAKIO_NETWORK");
}

/// One run of the binary inside `script`; killed and reaped when dropped, whatever happened.
pub struct Run {
    child: Child,
    dir: PathBuf,
    config: PathBuf,
    out: PathBuf,
    size: (u16, u16),
}

impl Drop for Run {
    fn drop(&mut self) {
        if let Some(pid) = app_pid(&self.config) {
            let _ = Command::new("kill").arg("-KILL").arg(pid.to_string()).status();
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        let deadline = Instant::now() + Duration::from_secs(10);
        while app_pid(&self.config).is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Run {
    /// Start the binary with config file `config` in a pseudo terminal of `size` (columns,
    /// rows), recording what it draws in `out`; `None` when `script` cannot start.
    pub fn spawn(dir: &Path, config: &Path, out: &Path, env: &[(&str, &str)], size: (u16, u16)) -> Option<Self> {
        Self::spawn_with(dir, config, out, env, size, "")
    }

    /// [`Run::spawn`] with `command` (`login`) before the options.
    pub fn spawn_with(
        dir: &Path,
        config: &Path,
        out: &Path,
        env: &[(&str, &str)],
        size: (u16, u16),
        command: &str,
    ) -> Option<Self> {
        let child = start(dir, config, out, env, size, command)?;
        Some(Run { child, dir: dir.to_path_buf(), config: config.to_path_buf(), out: out.to_path_buf(), size })
    }

    /// The screen now.
    pub fn output(&self) -> String {
        let (cols, rows) = self.size;
        let typescript = std::fs::read(&self.out).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        screen(&typescript, usize::from(rows), usize::from(cols))
    }

    pub fn wait_for(&self, what: &str, done: impl Fn(&str) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !done(&self.output()) {
            if Instant::now() >= deadline {
                let log = std::fs::read_to_string(self.dir.join("state").join("slakio").join("errors.log"))
                    .unwrap_or_default();
                panic!("timed out waiting for {what}: {}\nerrors.log: {log}", self.output());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn type_bytes(&mut self, bytes: &[u8]) {
        let stdin = self.child.stdin.as_mut().expect("stdin");
        stdin.write_all(bytes).unwrap();
        stdin.flush().unwrap();
    }

    /// `:qa`, and the binary gone.
    pub fn quit(mut self) {
        self.type_bytes(b":qa\r");
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.child.try_wait().ok().flatten().is_none() {
            assert!(Instant::now() < deadline, "the binary did not quit");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// The screen as the terminal shows it after `t`: a small emulator of what the binary writes
/// (cursor moves, clears, text; styles and modes ignored). Ratatui writes only the cells that
/// changed, so a text is looked for on the screen, never in the stream.
fn screen(t: &str, rows: usize, cols: usize) -> String {
    let mut grid = vec![vec![' '; cols]; rows];
    let (mut r, mut c) = (0usize, 0usize);
    let mut chars = t.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => match chars.next() {
                Some('[') => {
                    let mut params = String::new();
                    let mut fin = ' ';
                    for n in chars.by_ref() {
                        if ('@'..='~').contains(&n) {
                            fin = n;
                            break;
                        }
                        params.push(n);
                    }
                    let nums: Vec<usize> = params.split(';').map(|p| p.parse().unwrap_or(1)).collect();
                    match fin {
                        'H' | 'f' if !params.starts_with('?') => {
                            r = nums.first().copied().unwrap_or(1).saturating_sub(1).min(rows - 1);
                            c = nums.get(1).copied().unwrap_or(1).saturating_sub(1).min(cols - 1);
                        }
                        'J' if params == "2" => grid = vec![vec![' '; cols]; rows],
                        'h' if params == "?1049" => grid = vec![vec![' '; cols]; rows],
                        _ => {}
                    }
                }
                Some(']') => {
                    for n in chars.by_ref() {
                        if n == '\u{7}' {
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' => c = 0,
            '\n' => r = (r + 1).min(rows - 1),
            ch if ch.is_control() => {}
            ch => {
                let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if w == 0 {
                    continue;
                }
                if c < cols {
                    grid[r][c] = ch;
                    if w == 2 && c + 1 < cols {
                        grid[r][c + 1] = ' ';
                    }
                }
                c += w;
            }
        }
    }
    grid.into_iter().map(|row| row.into_iter().collect::<String>()).collect::<Vec<_>>().join("\n")
}

/// Start the binary in a pseudo terminal of `size`, recording what it draws in `out`.
fn start(
    dir: &Path,
    config: &Path,
    out: &Path,
    env: &[(&str, &str)],
    size: (u16, u16),
    command: &str,
) -> Option<Child> {
    let line = format!(
        "stty rows {} cols {}; exec '{}' {command} --config '{}'",
        size.1,
        size.0,
        env!("CARGO_BIN_EXE_slakio"),
        config.display()
    );
    let mut cmd = Command::new("script");
    if cfg!(target_os = "linux") {
        cmd.args(["-q", "-f", "-e", "-c", &line]).arg(out);
    } else {
        cmd.args(["-q", "-F"]).arg(out).args(["sh", "-c", &line]);
    }
    dirs(&mut cmd, dir);
    cmd.envs(env.iter().copied()).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
    match cmd.spawn() {
        Ok(child) => Some(child),
        Err(e) if std::env::var(REQUIRE_PTY).is_ok_and(|v| v == "1") => {
            panic!("`script` could not start ({e}), and {REQUIRE_PTY}=1 requires it")
        }
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "SKIPPED: `script` could not start: {e}");
            None
        }
    }
}

/// The binary's process: its command line names this run's config file, and it is slakio.
fn app_pid(config: &Path) -> Option<u32> {
    let out = Command::new("pgrep").arg("-f").arg(config.display().to_string()).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.trim().parse::<u32>().ok()).find(|pid| {
        Command::new("ps")
            .args(["-o", "comm=", "-p", &pid.to_string()])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim().ends_with("slakio"))
    })
}
