//! Reading back through the 10,000-message channel with the real binary in a pseudo terminal,
//! against the tests' fake Slack: `gg` pressed again and again sends a few paced requests, never
//! a burst, and the request count stays far under the governor's caps; a page Slack refuses with
//! 429 shows a row, is asked for again by itself after `Retry-After`, and paging goes on; on a
//! slow network a page on its way shows a loading row. Unix only.
//!
//! When `script` cannot start, each test prints a visible `SKIPPED` line and passes; with
//! `SLAKIO_REQUIRE_PTY=1` (CI on Linux and macOS) it fails instead.
#![cfg(unix)]

#[path = "support/pty.rs"]
mod pty;

use pty::{COOKIE, Run, TOKEN};
use slakio_fake::{FakeSlack, Fault};
use slakio_world::World;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const SIZE: (u16, u16) = (120, 40);

struct Machine {
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
    fn new() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let world = World::new(7);
        let fake = rt.block_on(FakeSlack::start(World::new(7))).unwrap();
        let dir = std::env::temp_dir().join(format!("slakio-paging-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = dir.join("config").join("slakio");
        std::fs::create_dir_all(&config).unwrap();
        let ws = &world.snapshot().workspaces[0];
        let file = format!(
            "[[workspace]]\nteam = \"{}\"\nname = \"Fake\"\nhost = \"example.slack.com\"\nuser = \"{}\"\n",
            ws.id, ws.me
        );
        std::fs::write(config.join("workspaces.toml"), file).unwrap();
        std::fs::write(dir.join("config.toml"), "icons = \"off\"\ncache = \"off\"\n").unwrap();
        Self { _rt: rt, fake, world, dir }
    }

    fn start(&self) -> Option<Run> {
        let ws = &self.world.snapshot().workspaces[0];
        let creds = format!("{}|{TOKEN}|{COOKIE}", ws.id);
        let addr = self.fake.addr().to_string();
        let env = [("SLAKIO_TEST_CREDENTIALS", creds.as_str()), ("SLAKIO_TEST_SLACK_ADDR", addr.as_str())];
        Run::spawn(&self.dir, &self.dir.join("config.toml"), &self.dir.join("typescript"), &env, SIZE)
    }

    fn calls(&self, method: &str) -> usize {
        self.fake.methods().iter().filter(|m| *m == method).count()
    }
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Click the list row that shows `name`.
fn click(run: &mut Run, name: &str) {
    let screen = run.output();
    let (row, line) = screen.lines().enumerate().find(|(_, l)| l.contains(name)).expect("the row on screen");
    let col = line[..line.find(name).unwrap()].chars().count() + 1;
    let (x, y) = (col + 1, row + 1);
    run.type_bytes(format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m").as_bytes());
}

#[test]
fn gg_in_the_10k_channel_is_paced_and_recovers_after_a_429() {
    let m = Machine::new();
    let Some(mut run) = m.start() else { return };
    run.wait_for("the list", |t| t.contains("big-history"));
    click(&mut run, "big-history");
    wait_until("the channel opened", || m.calls("conversations.view") >= 1);
    run.wait_for("its messages", |t| t.contains("#big-history") && t.contains("Message #big-history"));
    run.type_bytes(b"k");
    // Forty `gg` within a second.
    let t0 = Instant::now();
    for _ in 0..40 {
        run.type_bytes(b"gg");
        std::thread::sleep(Duration::from_millis(25));
    }
    std::thread::sleep(Duration::from_millis(500));
    let paced = 1 + (t0.elapsed().as_millis() / 1500) as usize + 1;
    let asked = m.calls("conversations.history");
    assert!((1..=paced).contains(&asked), "{asked} older pages for 40 gestures in {:?}", t0.elapsed());
    // Slack refuses the next one with 429 and two seconds to wait.
    m.fake.fail_next("conversations.history", Fault::Status { status: 429, retry_after: Some("2".into()) });
    std::thread::sleep(Duration::from_millis(1600));
    let before = m.calls("conversations.history");
    run.type_bytes(b"gg");
    run.wait_for("the failed row", |t| t.contains("Older messages not loaded"));
    wait_until("asked again by itself", || m.calls("conversations.history") > before + 1);
    run.wait_for("the row gone", |t| !t.contains("Older messages not loaded"));
    // And paging goes on: no stall.
    std::thread::sleep(Duration::from_millis(1600));
    let n = m.calls("conversations.history");
    run.type_bytes(b"gg");
    wait_until("one more page", || m.calls("conversations.history") > n);
    // On a slow network the page on its way shows a row above the oldest message.
    m.fake.set_delay(Duration::from_secs(3));
    std::thread::sleep(Duration::from_millis(1600));
    run.type_bytes(b"gg");
    run.wait_for("the loading row", |t| t.contains("Loading older messages…"));
    run.wait_for("the page landed", |t| !t.contains("Loading older messages…"));
    m.fake.set_delay(Duration::ZERO);
    let total = m.fake.log().len();
    assert!(total < 60, "far under the per-minute cap: {total} requests");
    run.quit();
}
