//! The local cache with the real binary in a pseudo terminal (`script`), signed in to the tests'
//! fake Slack: a first run keeps the workspace and the conversation it opened; the next run
//! draws both from the cache with the network off — the fake gets no request at all — or while
//! the fake never answers; a cache that is not a database is set up again with one notice;
//! `cache = "off"` keeps nothing; `slakio logout` removes it. The cache directory's exclusion
//! from backups goes to a stand-in for `tmutil` (Time Machine is never touched). Unix only.
//!
//! When `script` cannot start, each test prints a visible `SKIPPED` line and passes; with
//! `SLAKIO_REQUIRE_PTY=1` (CI on Linux and macOS) it fails instead.
#![cfg(unix)]

use slakio_fake::{FakeSlack, Fault};
use slakio_world::World;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const REQUIRE_PTY: &str = "SLAKIO_REQUIRE_PTY";
/// Synthetic credentials (spelled in parts so no file holds a token-shaped string).
const TOKEN: &str = concat!("xo", "xc-1-2-TESTTOKEN");
const COOKIE: &str = concat!("xo", "xd-TESTCOOKIE");

/// The fake Slack and the scratch directory every run of one test shares (as one machine).
struct Machine {
    /// Runs the fake while the test waits.
    _rt: tokio::runtime::Runtime,
    fake: FakeSlack,
    world: World,
    dir: PathBuf,
}

impl Drop for Machine {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Machine {
    fn new(tag: &str) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let world = World::new(7);
        let fake = rt.block_on(FakeSlack::start(World::new(7))).unwrap();
        let dir = std::env::temp_dir().join(format!("slakio-cache-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = dir.join("config").join("slakio");
        std::fs::create_dir_all(&config).unwrap();
        let ws = &world.snapshot().workspaces[0];
        let file = format!(
            "[[workspace]]\nteam = \"{}\"\nname = \"Fake\"\nhost = \"example.slack.com\"\nuser = \"{}\"\n",
            ws.id, ws.me
        );
        std::fs::write(config.join("workspaces.toml"), file).unwrap();
        // Asked nothing at the first start (the icons question).
        std::fs::write(dir.join("config.toml"), "icons = \"off\"\n").unwrap();
        // The stand-in for tmutil: it records its arguments.
        let tmutil = dir.join("tmutil");
        std::fs::write(
            &tmutil,
            format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n", dir.join("tmutil.log").display()),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmutil, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self { _rt: rt, fake, world, dir }
    }

    fn cache_dir(&self) -> PathBuf {
        self.dir.join("cache").join("slakio")
    }

    /// The cache's own directory in it, which slakio makes (and alone changes).
    fn store_dir(&self) -> PathBuf {
        self.cache_dir().join("slakio-store")
    }

    fn requests(&self) -> usize {
        self.fake.log().len()
    }

    fn tmutil_calls(&self) -> String {
        std::fs::read_to_string(self.dir.join("tmutil.log")).unwrap_or_default()
    }

    /// Start the binary, signed in to the fake, with `env` set; `None` when `script` cannot start.
    fn start(&self, run: usize, env: &[(&str, &str)]) -> Option<Run> {
        let ws = &self.world.snapshot().workspaces[0];
        let creds = format!("{}|{TOKEN}|{COOKIE}", ws.id);
        let addr = self.fake.addr().to_string();
        let tmutil = self.dir.join("tmutil").display().to_string();
        let mut all: Vec<(&str, &str)> = vec![
            ("SLAKIO_TEST_CREDENTIALS", &creds),
            ("SLAKIO_TEST_SLACK_ADDR", &addr),
            ("SLAKIO_TEST_TMUTIL", &tmutil),
        ];
        all.extend_from_slice(env);
        let out = self.dir.join(format!("typescript-{run}"));
        // A config file of its own, so each run's process is told apart from another's.
        let config = self.dir.join(format!("config-{run}.toml"));
        std::fs::copy(self.dir.join("config.toml"), &config).unwrap();
        let child = start(&self.dir, &config, &out, &all)?;
        Some(Run { child, dir: self.dir.clone(), config, out })
    }

    /// `slakio logout`, as a user runs it.
    fn logout(&self) -> std::process::Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_slakio"));
        cmd.arg("logout");
        dirs(&mut cmd, &self.dir);
        cmd.output().unwrap()
    }

    /// The newest message of the conversation the fake was asked to open, as words to look for.
    fn opened_words(&self) -> Vec<String> {
        let log = self.fake.log();
        let view = log.iter().find(|l| l.method() == "conversations.view").expect("a conversation was opened");
        let channel = view.fields().into_iter().find(|(k, _)| *k == "channel").unwrap().1.to_string();
        let (messages, _) = self.world.history(&slakio_core::model::ConversationId::new(channel), None, 28);
        let text = messages.last().unwrap().text.unsanitized().to_string();
        // A short text whole, else its longer plain words (drawn without a style inside them).
        let plain_text = text.len() <= 40 && text.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ');
        let words: Vec<String> = if plain_text {
            vec![text.clone()]
        } else {
            text.split_whitespace()
                .filter(|w| w.len() >= 5 && w.chars().all(|c| c.is_ascii_alphabetic()))
                .map(String::from)
                .collect()
        };
        assert!(!words.is_empty(), "a word to look for in {text:?}");
        words
    }
}

/// Every directory the binary could read or write inside `dir`; credentials in memory.
fn dirs(cmd: &mut Command, dir: &Path) {
    cmd.env("SLAKIO_SECRET_STORE", "memory")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("LC_ALL", "en_US.UTF-8")
        .env("TERM", "xterm-256color")
        .env_remove("TMUX")
        .env_remove("SLAKIO_NETWORK");
}

/// One run of the binary inside `script`; killed and reaped when dropped, whatever happened.
struct Run {
    child: Child,
    dir: PathBuf,
    config: PathBuf,
    out: PathBuf,
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
    /// The screen now.
    fn output(&self) -> String {
        screen(&std::fs::read(&self.out).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default())
    }

    fn wait_for(&self, what: &str, done: impl Fn(&str) -> bool) {
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

    fn type_bytes(&mut self, bytes: &[u8]) {
        let stdin = self.child.stdin.as_mut().expect("stdin");
        stdin.write_all(bytes).unwrap();
        stdin.flush().unwrap();
    }

    /// `:qa`, and the binary gone.
    fn quit(mut self) {
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
fn screen(t: &str) -> String {
    let (rows, cols) = (30usize, 100usize);
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

/// Start the binary in a 100×30 pseudo terminal, recording what it draws in `out`.
fn start(dir: &Path, config: &Path, out: &Path, env: &[(&str, &str)]) -> Option<Child> {
    let line =
        format!("stty rows 30 cols 100; exec '{}' --config '{}'", env!("CARGO_BIN_EXE_slakio"), config.display());
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
            let _ = writeln!(std::io::stderr(), "SKIPPED cache_flows: `script` could not start: {e}");
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

/// A first run with the network on: boots, opens the first conversation, quits. The words of the
/// newest message it showed.
fn first_run(m: &Machine) -> Option<Vec<String>> {
    let mut run = m.start(1, &[])?;
    run.wait_for("the workspace's channels", |t| t.contains("backend"));
    run.type_bytes(b"\r");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !m.fake.methods().iter().any(|x| x == "conversations.view") {
        assert!(Instant::now() < deadline, "the conversation was never asked for: {:?}", m.fake.methods());
        std::thread::sleep(Duration::from_millis(50));
    }
    let words = m.opened_words();
    run.wait_for("the conversation's newest message", |t| words.iter().any(|w| t.contains(w.as_str())));
    run.quit();
    Some(words)
}

/// The kept cache made unreadable (not a database).
fn damage(m: &Machine) {
    std::fs::write(m.store_dir().join("cache.sqlite3"), b"not a database ".repeat(500)).unwrap();
    for side in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(m.store_dir().join(format!("cache.sqlite3{side}")));
    }
}

/// A stand-in for tmutil that records its arguments and fails.
fn failing_tmutil(m: &Machine) {
    use std::os::unix::fs::PermissionsExt;
    let log = m.dir.join("tmutil.log");
    std::fs::write(m.dir.join("tmutil"), format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\nexit 1\n", log.display()))
        .unwrap();
    std::fs::set_permissions(m.dir.join("tmutil"), std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn a_warm_start_with_the_network_off_draws_from_the_cache_and_asks_slack_nothing() {
    let m = Machine::new("offline");
    let Some(words) = first_run(&m) else { return };
    assert!(m.store_dir().join("cache.sqlite3").is_file(), "the first run kept a cache");
    let excluded = m.tmutil_calls();
    assert_eq!(excluded, format!("addexclusion\n{}\n", m.store_dir().display()), "excluded from backups once");
    let before = m.requests();
    let Some(mut run) = m.start(2, &[("SLAKIO_NETWORK", "off")]) else { return };
    run.wait_for("the cached list, marked as cached", |t| t.contains("backend") && t.contains("read only · cached"));
    run.wait_for("why it is not refreshed", |t| t.contains("Offline"));
    assert!(!run.output().contains("Could not load"), "the cached list stays: {}", run.output());
    run.type_bytes(b"\r");
    run.wait_for("the cached messages", |t| words.iter().any(|w| t.contains(w.as_str())));
    assert!(!run.output().contains("Loading messages"), "{}", run.output());
    run.quit();
    assert_eq!(m.requests(), before, "not one request with the network off: {:?}", m.fake.methods());
    assert_eq!(m.tmutil_calls(), excluded, "the exclusion is remembered, not asked again");
}

#[test]
fn a_warm_start_draws_from_the_cache_while_slack_has_not_answered() {
    let m = Machine::new("hang");
    let Some(words) = first_run(&m) else { return };
    for method in ["client.userBoot", "conversations.view"] {
        m.fake.fail_next(method, Fault::Hang);
    }
    let before = m.requests();
    let Some(mut run) = m.start(2, &[]) else { return };
    run.wait_for("the cached list", |t| t.contains("backend") && t.contains("cached"));
    run.type_bytes(b"\r");
    run.wait_for("the cached messages", |t| words.iter().any(|w| t.contains(w.as_str())));
    let methods: Vec<String> = m.fake.methods()[before..].to_vec();
    assert!(methods.contains(&"client.userBoot".to_string()), "the refresh was asked for: {methods:?}");
    assert!(!methods.contains(&"client.counts".to_string()), "and never answered (counts come after it): {methods:?}");
    run.quit();
}

#[test]
fn a_warm_start_with_the_network_on_is_refreshed_and_no_longer_marked_cached() {
    let m = Machine::new("online");
    let Some(words) = first_run(&m) else { return };
    let before = m.requests();
    let Some(mut run) = m.start(2, &[]) else { return };
    run.wait_for("the refreshed list", |t| t.contains("backend") && t.contains("read only") && !t.contains("cached"));
    run.type_bytes(b"\r");
    run.wait_for("the refreshed messages", |t| words.iter().any(|w| t.contains(w.as_str())) && !t.contains("cached"));
    run.quit();
    let methods: Vec<String> = m.fake.methods()[before..].to_vec();
    assert!(
        methods.contains(&"client.userBoot".to_string()) && methods.contains(&"conversations.view".to_string()),
        "{methods:?}"
    );
}

#[test]
fn a_second_slakio_draws_from_the_cache_without_writing_it_and_says_so() {
    let m = Machine::new("second");
    if first_run(&m).is_none() {
        return;
    }
    let Some(first) = m.start(2, &[]) else { return };
    first.wait_for("the first one up", |t| t.contains("backend"));
    let Some(second) = m.start(3, &[]) else { return };
    second.wait_for("the notice", |t| t.contains("Another slakio runs: cache read only"));
    second.wait_for("the cached list", |t| t.contains("backend"));
    second.quit();
    first.quit();
}

#[test]
fn a_failed_exclusion_from_backups_is_said_once_and_tried_again() {
    let m = Machine::new("tmfail");
    failing_tmutil(&m);
    let Some(run) = m.start(1, &[]) else { return };
    run.wait_for("the notice", |t| t.contains("Cache not kept out of Time Machine"));
    run.wait_for("the workspace", |t| t.contains("backend"));
    run.quit();
    let Some(run) = m.start(2, &[]) else { return };
    run.wait_for("the start", |t| t.contains("backend"));
    std::thread::sleep(Duration::from_millis(500));
    assert!(!run.output().contains("Time Machine"), "said once: {}", run.output());
    run.quit();
    assert_eq!(m.tmutil_calls().matches("addexclusion").count(), 2, "tried again: {}", m.tmutil_calls());
}

#[test]
fn a_cache_that_is_not_a_database_is_started_again_with_one_notice() {
    let m = Machine::new("corrupt");
    if first_run(&m).is_none() {
        return;
    }
    damage(&m);
    let Some(run) = m.start(2, &[]) else { return };
    run.wait_for("the notice", |t| t.contains("Cache unreadable: started again"));
    run.wait_for("the workspace, fetched again", |t| t.contains("backend"));
    run.quit();
    let log = std::fs::read_to_string(m.dir.join("state").join("slakio").join("errors.log")).unwrap();
    assert!(log.contains("cache"), "{log}");
    // Set up again and written: the next start draws from it.
    let Some(run) = m.start(3, &[("SLAKIO_NETWORK", "off")]) else { return };
    run.wait_for("the cached list", |t| t.contains("backend") && t.contains("cached"));
    assert!(!run.output().contains("Cache unreadable"));
    run.quit();
}

#[test]
fn logout_removes_the_cache_and_cache_off_keeps_nothing() {
    let m = Machine::new("logout");
    if first_run(&m).is_none() {
        return;
    }
    let out = m.logout();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let left: Vec<_> = std::fs::read_dir(m.cache_dir()).unwrap().flatten().map(|e| e.file_name()).collect();
    assert!(left.is_empty(), "nothing of the cache is left: {left:?}");

    let m = Machine::new("off");
    std::fs::write(m.dir.join("config.toml"), "icons = \"off\"\ncache = \"off\"\n").unwrap();
    if first_run(&m).is_none() {
        return;
    }
    assert!(!m.store_dir().join("cache.sqlite3").exists(), "cache = off keeps nothing");
    assert_eq!(m.tmutil_calls(), "", "and excludes nothing");
}

#[test]
fn nothing_secret_is_kept_in_the_cache() {
    let m = Machine::new("secret");
    if first_run(&m).is_none() {
        return;
    }
    let files: Vec<_> = std::fs::read_dir(m.store_dir()).unwrap().flatten().collect();
    assert!(files.iter().any(|e| e.file_name() == "cache.sqlite3"), "the cache's files are looked at");
    for e in files {
        let bytes = std::fs::read(e.path()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("TESTTOKEN") && !text.contains("TESTCOOKIE"), "{}", e.path().display());
    }
}

/// The words of the boot that fails with the network off.
const OFFLINE: &str = "Offline: Slack is not being asked";

#[test]
fn a_cache_notice_is_seen_after_the_offline_warning_that_comes_at_the_same_time() {
    let m = Machine::new("offline-rebuilt");
    if first_run(&m).is_none() {
        return;
    }
    damage(&m);
    let Some(run) = m.start(2, &[("SLAKIO_NETWORK", "off")]) else { return };
    run.wait_for("why nothing is fetched", |t| t.contains(OFFLINE));
    run.wait_for("the cache's notice, after it", |t| t.contains("Cache unreadable: started again"));
    run.quit();
}

#[test]
fn a_second_slakio_offline_still_says_it_reads_the_cache_only() {
    let m = Machine::new("second-offline");
    if first_run(&m).is_none() {
        return;
    }
    let Some(first) = m.start(2, &[]) else { return };
    first.wait_for("the first one up", |t| t.contains("backend"));
    let Some(second) = m.start(3, &[("SLAKIO_NETWORK", "off")]) else { return };
    second.wait_for("why nothing is fetched", |t| t.contains(OFFLINE));
    second.wait_for("the cache's notice, after it", |t| t.contains("Another slakio runs: cache read only"));
    second.quit();
    first.quit();
}

#[test]
fn two_cache_notices_at_once_are_each_seen() {
    let m = Machine::new("two-notices");
    if first_run(&m).is_none() {
        return;
    }
    damage(&m);
    failing_tmutil(&m);
    let Some(run) = m.start(2, &[]) else { return };
    run.wait_for("the first notice", |t| t.contains("Cache unreadable: started again"));
    run.wait_for("the second one, after it", |t| t.contains("Cache not kept out of Time Machine"));
    run.quit();
}

#[test]
fn another_user_of_the_workspace_never_sees_the_first_ones_cache() {
    let m = Machine::new("other-user");
    if first_run(&m).is_none() {
        return;
    }
    let file = m.dir.join("config").join("slakio").join("workspaces.toml");
    let me = m.world.snapshot().workspaces[0].me.to_string();
    let text = std::fs::read_to_string(&file).unwrap().replace(&format!("user = \"{me}\""), "user = \"UOTHER\"");
    assert!(text.contains("UOTHER"));
    std::fs::write(&file, text).unwrap();
    let Some(run) = m.start(2, &[("SLAKIO_NETWORK", "off")]) else { return };
    run.wait_for("why nothing is fetched", |t| t.contains(OFFLINE));
    std::thread::sleep(Duration::from_millis(500));
    assert!(!run.output().contains("cached") && !run.output().contains("backend"), "{}", run.output());
    run.quit();
}

#[test]
fn logout_removes_the_cache_even_when_the_sign_in_could_not_be_removed() {
    let m = Machine::new("logout-failed");
    if first_run(&m).is_none() {
        return;
    }
    // A workspaces file that cannot be read: the credentials step fails.
    std::fs::write(m.dir.join("config").join("slakio").join("workspaces.toml"), "[[[ not toml").unwrap();
    let out = m.logout();
    assert!(!out.status.success());
    let left: Vec<_> = std::fs::read_dir(m.cache_dir()).unwrap().flatten().map(|e| e.file_name()).collect();
    assert!(left.is_empty(), "the cache went all the same: {left:?}");
}

#[test]
fn logout_while_a_slakio_runs_removes_the_cache_and_says_to_quit_it() {
    let m = Machine::new("logout-running");
    if first_run(&m).is_none() {
        return;
    }
    let Some(run) = m.start(2, &[]) else { return };
    run.wait_for("it up", |t| t.contains("backend"));
    let out = m.logout();
    let said = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(!out.status.success(), "{said}");
    assert!(said.contains("A slakio is still running: quit it"), "{said}");
    assert!(!m.store_dir().join("cache.sqlite3").exists(), "removed all the same");
    run.quit();
}
