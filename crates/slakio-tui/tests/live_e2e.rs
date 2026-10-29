//! Live updates with the real binary in a pseudo terminal, against the tests' fake Slack, at
//! 120x40 and 80x24: once the workspace started its socket opens and the status line says so; a
//! message posted shows in the open conversation, its edit replaces it, its deletion takes it
//! away; a dropped socket is said, marked in the status line until it opens again by itself, and
//! the open conversation is read again (one page, nothing more); with the network off no socket ever opens, nor with live updates
//! switched off (the rest works). Unix only.
//!
//! When `script` cannot start, each test prints a visible `SKIPPED` line and passes; with
//! `SLAKIO_REQUIRE_PTY=1` (CI on Linux and macOS) it fails instead.
#![cfg(unix)]

#[path = "support/pty.rs"]
mod pty;

use pty::{COOKIE, Run, TOKEN};
use serde_json::json;
use slakio_fake::FakeSlack;
use slakio_world::World;
use std::path::PathBuf;
use std::time::{Duration, Instant};

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
    /// A machine of its own for test `name` (tests run at once in one process).
    fn new(name: &str) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let world = World::new(7);
        let fake = rt.block_on(FakeSlack::start(World::new(7))).unwrap();
        let dir = std::env::temp_dir().join(format!("slakio-live-{}-{name}", std::process::id()));
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

    fn start(&self, size: (u16, u16), extra: &[(&str, &str)]) -> Option<Run> {
        let ws = &self.world.snapshot().workspaces[0];
        let creds = format!("{}|{TOKEN}|{COOKIE}", ws.id);
        let addr = self.fake.addr().to_string();
        let mut env = vec![("SLAKIO_TEST_CREDENTIALS", creds.as_str()), ("SLAKIO_TEST_SLACK_ADDR", addr.as_str())];
        env.extend_from_slice(extra);
        Run::spawn(&self.dir, &self.dir.join("config.toml"), &self.dir.join("typescript"), &env, size)
    }

    fn calls(&self, method: &str) -> usize {
        self.fake.methods().iter().filter(|m| *m == method).count()
    }

    fn channel(&self, name: &str) -> String {
        let s = self.world.snapshot();
        s.conversations.iter().find(|c| c.workspace == s.workspaces[0].id && c.name == name).unwrap().id.to_string()
    }

    fn other(&self) -> String {
        let s = self.world.snapshot();
        let ws = &s.workspaces[0];
        s.users.iter().find(|u| u.workspace == ws.id && u.id != ws.me && !u.bot).unwrap().id.to_string()
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
fn messages_arrive_live_and_a_dropped_socket_comes_back_and_catches_up() {
    for size in [(120, 40), (80, 24)] {
        let m = Machine::new(&format!("live-{}x{}", size.0, size.1));
        let Some(mut run) = m.start(size, &[]) else { return };
        run.wait_for("the list", |t| t.contains("deploys"));
        run.wait_for("live updates on", |t| t.contains("Live updates on"));
        assert_eq!(m.fake.upgrades().len(), 1);
        click(&mut run, "deploys");
        wait_until("the channel opened", || m.calls("conversations.view") >= 1);
        run.wait_for("its messages", |t| t.contains("Message #deploys"));
        let (c, who) = (m.channel("deploys"), m.other());
        m.fake.push(
            &json!({"type": "message", "channel": c, "user": who, "text": "live hello", "ts": "1900000000.000100"}),
        );
        run.wait_for("the new message", |t| t.contains("live hello"));
        m.fake.push(&json!({"type": "message", "subtype": "message_changed", "channel": c, "hidden": true,
            "message": {"user": who, "text": "live hello, edited", "ts": "1900000000.000100",
                "edited": {"ts": "1900000001.000000"}}}));
        run.wait_for("the edit", |t| t.contains("live hello, edited"));
        m.fake.push(&json!({"type": "message", "subtype": "message_deleted", "channel": c,
            "deleted_ts": "1900000000.000100"}));
        run.wait_for("the deletion", |t| !t.contains("live hello"));
        // The network goes away: said, then back by itself, and the open conversation read again.
        let before = m.fake.log().len();
        m.fake.refuse_sockets(2);
        m.fake.drop_sockets();
        run.wait_for("lost", |t| t.contains("Live updates lost: reconnecting") && t.contains("live off"));
        run.wait_for("back", |t| t.contains("Live updates back"));
        run.wait_for("the mark gone", |t| !t.contains("live off"));
        wait_until("caught up", || m.calls("conversations.history") == 1);
        std::thread::sleep(Duration::from_millis(500));
        let after: Vec<String> = m.fake.methods()[before..].to_vec();
        assert!(after.len() <= 2, "one page (and its names at most): {after:?}");
        assert_eq!(m.fake.upgrades().len(), 4, "two refused, then open again");
        assert_eq!(m.calls("conversations.view"), 1, "not opened again");
        run.quit();
    }
}

#[test]
fn with_the_network_off_no_socket_ever_opens() {
    let m = Machine::new("off");
    let Some(run) = m.start((120, 40), &[("SLAKIO_NETWORK", "off")]) else { return };
    run.wait_for("the screen", |t| t.contains("read only") || t.contains("Offline"));
    std::thread::sleep(Duration::from_secs(3));
    assert!(m.fake.upgrades().is_empty(), "no socket");
    assert!(m.fake.log().is_empty(), "no request");
    assert!(!run.output().contains("Live updates") && !run.output().contains("live off"), "{}", run.output());
    run.quit();
}

#[test]
fn with_live_updates_off_no_socket_opens_and_the_rest_works() {
    let m = Machine::new("live-off");
    std::fs::write(m.dir.join("config.toml"), "icons = \"off\"\ncache = \"off\"\nlive = \"off\"\n").unwrap();
    let Some(mut run) = m.start((120, 40), &[]) else { return };
    run.wait_for("the list", |t| t.contains("deploys"));
    click(&mut run, "deploys");
    wait_until("the channel opened", || m.calls("conversations.view") >= 1);
    run.wait_for("its messages", |t| t.contains("Message #deploys"));
    std::thread::sleep(Duration::from_secs(2));
    assert!(m.fake.upgrades().is_empty(), "no socket");
    assert!(!run.output().contains("Live updates") && !run.output().contains("live off"), "{}", run.output());
    run.quit();
}
