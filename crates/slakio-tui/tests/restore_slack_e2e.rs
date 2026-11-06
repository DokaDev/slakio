//! A saved layout opened again by the real binary in a pseudo terminal, signed in to the tests'
//! fake Slack: every pane of it loads its conversation (two side by side, then four in a 2x2),
//! none says it could not load, and the fake is asked for each conversation once (and its
//! authors' names), nothing more. Unix only.
//!
//! When `script` cannot start, each test prints a visible `SKIPPED` line and passes; with
//! `SLAKIO_REQUIRE_PTY=1` (CI on Linux and macOS) it fails instead.
#![cfg(unix)]

#[path = "support/pty.rs"]
mod pty;

use pty::{COOKIE, Run, TOKEN};
use slakio_core::layout::saved::Saved;
use slakio_core::model::Conversation;
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
    fn new(tag: &str) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let world = World::new(7);
        let fake = rt.block_on(FakeSlack::start(World::new(7))).unwrap();
        let dir = std::env::temp_dir().join(format!("slakio-restore-slack-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = dir.join("config").join("slakio");
        std::fs::create_dir_all(&config).unwrap();
        let ws = &world.snapshot().workspaces[0];
        let file = format!(
            "[[workspace]]\nteam = \"{}\"\nname = \"Fake\"\nhost = \"example.slack.com\"\nuser = \"{}\"\n",
            ws.id, ws.me
        );
        std::fs::write(config.join("workspaces.toml"), file).unwrap();
        std::fs::write(dir.join("config.toml"), "icons = \"off\"\ncache = \"off\"\nlive = \"off\"\n").unwrap();
        Self { _rt: rt, fake, world, dir }
    }

    /// The first `n` channels of the first workspace with messages.
    fn channels(&self, n: usize) -> Vec<Conversation> {
        let s = self.world.snapshot();
        (s.conversations.iter())
            .filter(|c| c.workspace == s.workspaces[0].id && c.name.line().as_str().is_ascii())
            .filter(|c| matches!(c.kind, slakio_core::model::ConversationKind::Channel { .. }))
            .filter(|c| self.world.message_count(&c.id) > 0)
            .take(n)
            .cloned()
            .collect()
    }

    /// The layout file of the workspace signed in to, holding one tab of `channels` split as
    /// `tree`.
    fn save(&self, channels: &[Conversation], tree: &[&str]) {
        let ws = &self.world.snapshot().workspaces[0];
        let name = slakio_tui::layout_file::file_name(Some(&[(ws.id.clone(), ws.me.as_str().to_string())]));
        let tokens: Vec<String> = tree.iter().map(|t| format!("\"{t}\"")).collect();
        let mut text = format!(
            "version = 1\ncurrent = 0\n\n[[tab]]\nactive = 0\nzoomed = false\ntree = [{}]\n",
            tokens.join(", ")
        );
        for c in channels {
            text.push_str(&format!("\n[[tab.pane]]\nworkspace = \"{}\"\nconversation = \"{}\"\n", c.workspace, c.id));
        }
        let saved = Saved::parse(&text).expect("a layout that reads");
        assert_eq!(saved.tabs[0].panes.len(), channels.len());
        let state = self.dir.join("state").join("slakio");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join(name), text).unwrap();
    }

    fn start(&self, size: (u16, u16)) -> Option<Run> {
        let ws = &self.world.snapshot().workspaces[0];
        let creds = format!("{}|{TOKEN}|{COOKIE}", ws.id);
        let addr = self.fake.addr().to_string();
        let env = [("SLAKIO_TEST_CREDENTIALS", creds.as_str()), ("SLAKIO_TEST_SLACK_ADDR", addr.as_str())];
        Run::spawn(&self.dir, &self.dir.join("config.toml"), &self.dir.join("typescript"), &env, size)
    }

    fn calls(&self, method: &str) -> usize {
        self.fake.methods().iter().filter(|m| *m == method).count()
    }

    fn errors(&self) -> String {
        std::fs::read_to_string(self.dir.join("state").join("slakio").join("errors.log")).unwrap_or_default()
    }
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The layout of `n` channels split as `tree` opens with every one of them loaded.
fn every_pane_loads(n: usize, tree: &[&str]) {
    let m = Machine::new(&format!("{n}"));
    let channels = m.channels(n);
    assert_eq!(channels.len(), n);
    m.save(&channels, tree);
    let Some(run) = m.start((160, 40)) else { return };
    wait_until("every open", || m.calls("conversations.view") >= n);
    // Each pane shows its conversation's newest message.
    for c in &channels {
        let last = m.world.message(&c.id, m.world.message_count(&c.id) - 1).unwrap();
        let words: String = last.text.unsanitized().split_whitespace().next().unwrap_or_default().to_string();
        run.wait_for(&format!("#{} loaded", c.name.line().as_str()), |t| t.contains(&words));
    }
    std::thread::sleep(Duration::from_millis(500));
    let screen = run.output();
    assert!(!screen.contains("Could not load"), "{n}: {screen}");
    assert!(!m.errors().contains("superseded"), "{n}: {}", m.errors());
    let methods = m.fake.methods();
    let opens = m.calls("conversations.view");
    let reads: Vec<&String> = methods.iter().filter(|x| x.starts_with("conversations.")).collect();
    // The start's own requests, then the opens and their authors' names (a batch per page).
    let first = methods.iter().position(|x| x == "conversations.view").unwrap();
    let names = methods[first..].iter().filter(|x| *x == "users/info").count();
    assert_eq!(opens, n, "{n}: one open each: {methods:?}");
    assert_eq!(reads.len(), n, "{n}: no other read: {methods:?}");
    assert!(methods[first..].iter().all(|x| x == "conversations.view" || x == "users/info"), "{methods:?}");
    assert!(names <= n, "{n}: a batch of names per page at most: {methods:?}");
    run.quit();
}

#[test]
fn two_panes_side_by_side_both_load() {
    every_pane_loads(2, &["row 1 2 20 65535 20", "pane 0", "pane 1"]);
}

#[test]
fn four_panes_in_a_2x2_all_load() {
    every_pane_loads(
        4,
        &[
            "row 1 2 20 65535 20",
            "column 1 2 6 65535 6",
            "pane 0",
            "pane 1",
            "column 1 2 6 65535 6",
            "pane 2",
            "pane 3",
        ],
    );
}
