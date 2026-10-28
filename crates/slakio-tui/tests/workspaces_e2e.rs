//! Two workspaces signed in at once, with the real binary in a pseudo terminal against two fake
//! Slacks (one per workspace, each on its own address): both boot, the switcher moves between
//! them at 120x40 and 80x24; one that Slack refuses leaves the other working; signing in again
//! with a paste keeps the other workspace; signing out of one keeps the other, its cache part
//! too; each workspace's list comes back from the cache with the network off. Unix only.
#![cfg(unix)]

#[path = "support/pty.rs"]
mod pty;

use pty::{COOKIE, Run, TOKEN, dirs};
use slakio_core::model::ConversationKind;
use slakio_fake::{FakeSlack, Fault};
use slakio_world::World;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Two fakes (A and B) and the scratch directory every run of one test shares.
struct Machine {
    _rt: tokio::runtime::Runtime,
    fakes: [FakeSlack; 2],
    world: World,
    dir: PathBuf,
}

impl Drop for Machine {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const HOSTS: [&str; 2] = ["a-team.slack.com", "b-team.slack.com"];

impl Machine {
    /// Signed in to the workspaces `signed` (0: A, 1: B) as the workspaces file names them.
    fn new(tag: &str, signed: &[usize]) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let world = World::new(7);
        let fakes = [0, 1].map(|i| rt.block_on(FakeSlack::start_team(World::new(7), i)).unwrap());
        let dir = std::env::temp_dir().join(format!("slakio-two-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config").join("slakio")).unwrap();
        std::fs::write(dir.join("config.toml"), "icons = \"off\"\n").unwrap();
        let m = Self { _rt: rt, fakes, world, dir };
        let file: String = signed.iter().map(|&i| m.entry(i)).collect();
        std::fs::write(m.workspaces_file(), file).unwrap();
        m
    }

    fn entry(&self, i: usize) -> String {
        let ws = &self.world.snapshot().workspaces[i];
        format!(
            "[[workspace]]\nteam = \"{}\"\nname = \"{}\"\nhost = \"{}\"\nuser = \"{}\"\n",
            ws.id,
            self.name(i),
            HOSTS[i],
            ws.me
        )
    }

    fn workspaces_file(&self) -> PathBuf {
        self.dir.join("config").join("slakio").join("workspaces.toml")
    }

    fn name(&self, i: usize) -> String {
        self.world.snapshot().workspaces[i].name.line().into_string()
    }

    /// A channel of workspace `i` (its name as the list shows it).
    fn channel(&self, i: usize) -> String {
        let s = self.world.snapshot();
        let ws = &s.workspaces[i].id;
        let c =
            s.conversations.iter().find(|c| &c.workspace == ws && matches!(c.kind, ConversationKind::Channel { .. }));
        c.unwrap().name.line().into_string()
    }

    fn methods(&self, i: usize) -> Vec<String> {
        self.fakes[i].methods()
    }

    /// The environment that signs in to both fakes (credentials for both in memory).
    fn env(&self) -> (String, String) {
        let s = self.world.snapshot();
        let creds = format!("{}|{TOKEN}|{COOKIE};{}|{TOKEN}|{COOKIE}", s.workspaces[0].id, s.workspaces[1].id);
        let addrs = format!("{}={},{}={}", HOSTS[0], self.fakes[0].addr(), HOSTS[1], self.fakes[1].addr());
        (creds, addrs)
    }

    fn start(&self, run: usize, size: (u16, u16), extra: &[(&str, &str)]) -> Option<Run> {
        let (creds, addrs) = self.env();
        let mut env: Vec<(&str, &str)> = vec![("SLAKIO_TEST_CREDENTIALS", &creds), ("SLAKIO_TEST_SLACK_ADDR", &addrs)];
        env.extend_from_slice(extra);
        let config = self.dir.join(format!("config-{run}.toml"));
        std::fs::copy(self.dir.join("config.toml"), &config).unwrap();
        Run::spawn(&self.dir, &config, &self.dir.join(format!("typescript-{run}")), &env, size)
    }

    /// `slakio <args>` as a user runs it, `stdin` piped in, signing in to the fakes.
    fn cli(&self, args: &[&str], stdin: &str) -> Output {
        use std::io::Write;
        let (_, addrs) = self.env();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_slakio"));
        cmd.args(args).env("SLAKIO_TEST_SLACK_ADDR", addrs);
        dirs(&mut cmd, &self.dir);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    }

    fn file(&self) -> String {
        std::fs::read_to_string(self.workspaces_file()).unwrap_or_default()
    }
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Both workspaces boot and the switcher moves between them, at `size`.
fn both_boot_and_switch(tag: &str, size: (u16, u16)) {
    let m = Machine::new(tag, &[0, 1]);
    let Some(mut run) = m.start(1, size, &[]) else { return };
    let (a, b) = (m.channel(0), m.channel(1));
    run.wait_for("A's channels", |t| t.contains(&a));
    wait_until("both boots", || {
        m.methods(0).iter().any(|x| x == "client.userBoot") && m.methods(1).iter().any(|x| x == "client.userBoot")
    });
    // Each fake was asked only for its own workspace's start.
    assert_eq!(m.methods(0).iter().filter(|x| *x == "client.userBoot").count(), 1);
    assert_eq!(m.methods(1).iter().filter(|x| *x == "client.userBoot").count(), 1);
    let name_b = m.name(1);
    run.type_bytes(b" W");
    run.wait_for("the switcher with both workspaces", |t| t.contains(&m.name(0)) && t.contains(&name_b));
    run.type_bytes(b"j\r");
    run.wait_for("B's channels", |t| t.contains(&b) && !t.contains(&a));
    run.type_bytes(b" Wk\r");
    run.wait_for("A again", |t| t.contains(&a));
    run.quit();
}

#[test]
fn two_workspaces_boot_side_by_side_and_the_switcher_moves_between_them_at_120x40() {
    both_boot_and_switch("switch-120", (120, 40));
}

#[test]
fn two_workspaces_boot_side_by_side_and_the_switcher_moves_between_them_at_80x24() {
    both_boot_and_switch("switch-80", (80, 24));
}

#[test]
fn a_workspace_slack_refuses_leaves_the_other_working() {
    let m = Machine::new("one-refused", &[0, 1]);
    m.fakes[1].fail_next("client.userBoot", Fault::Api("invalid_auth".into()));
    let Some(mut run) = m.start(1, (120, 40), &[]) else { return };
    let a = m.channel(0);
    run.wait_for("A's channels", |t| t.contains(&a));
    let name_b = m.name(1);
    run.wait_for("whose start failed", |t| t.contains(&format!("{name_b}: Signed out")));
    run.type_bytes(b" Wj\r");
    run.wait_for("B's list says it could not be loaded", |t| t.contains("Could not load"));
    // A still opens a conversation.
    run.type_bytes(b" Wk\r");
    run.wait_for("A's channels", |t| t.contains(&a));
    run.type_bytes(b"\r");
    wait_until("A's conversation", || m.methods(0).iter().any(|x| x == "conversations.view"));
    assert!(!m.methods(1).iter().any(|x| x == "conversations.view"), "nothing asked of B");
    run.quit();
}

#[test]
fn a_hung_workspace_never_holds_the_other_back() {
    let m = Machine::new("one-hung", &[0, 1]);
    m.fakes[1].fail_next("client.userBoot", Fault::Hang);
    let Some(mut run) = m.start(1, (100, 30), &[]) else { return };
    let a = m.channel(0);
    run.wait_for("A's channels while B hangs", |t| t.contains(&a));
    run.type_bytes(b" Wj\r");
    run.wait_for("B still loading", |t| t.contains("Loading"));
    run.quit();
}

#[test]
fn signing_in_again_with_a_paste_keeps_the_other_workspace() {
    let m = Machine::new("relogin", &[0]);
    let paste = format!(
        "curl 'https://{}/api/client.counts' \\\n  -H 'cookie: b=1; d={COOKIE}' \\\n  --data-raw 'token={TOKEN}'",
        HOSTS[1]
    );
    let out = m.cli(&["login", "--paste"], &paste);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains(&format!("Signed in to {}", m.name(1))), "{}", text(&out));
    let file = m.file();
    let (ta, tb) = (&m.world.snapshot().workspaces[0].id, &m.world.snapshot().workspaces[1].id);
    let (at_a, at_b) = (file.find(&format!("team = \"{ta}\"")), file.find(&format!("team = \"{tb}\"")));
    assert!(at_a.is_some() && at_b.is_some() && at_a < at_b, "A kept, B added after it: {file}");
    // The same paste again: B is updated, not listed twice.
    assert!(m.cli(&["login"], &paste).status.success());
    assert_eq!(m.file().matches("[[workspace]]").count(), 2, "{}", m.file());
    // Started, both are listed and boot.
    let Some(mut run) = m.start(1, (120, 40), &[]) else { return };
    run.wait_for("A's channels", |t| t.contains(&m.channel(0)));
    run.type_bytes(b" Wj\r");
    run.wait_for("B's channels", |t| t.contains(&m.channel(1)));
    run.quit();
}

#[test]
fn signing_out_of_one_workspace_keeps_the_other_and_its_cache() {
    let m = Machine::new("logout-one", &[0, 1]);
    let Some(mut run) = m.start(1, (100, 30), &[]) else { return };
    let (a, b) = (m.channel(0), m.channel(1));
    run.wait_for("A's channels", |t| t.contains(&a));
    wait_until("B booted", || m.methods(1).iter().any(|x| x == "client.userBoot"));
    run.type_bytes(b" Wj\r");
    run.wait_for("B's channels", |t| t.contains(&b));
    run.quit();
    // Several signed in, none named: nothing is signed out, both are listed.
    let out = m.cli(&["logout"], "");
    assert!(!out.status.success());
    let said = text(&out);
    assert!(said.contains("Signed in to several workspaces") && said.contains(&m.name(0)), "{said}");
    assert!(said.contains(&format!("{} ({})", m.name(1), HOSTS[1])), "{said}");
    assert_eq!(m.file().matches("[[workspace]]").count(), 2);
    let out = m.cli(&["logout", "nothing-like-it"], "");
    assert!(!out.status.success() && text(&out).contains("Not signed in to a workspace named"), "{}", text(&out));
    // B by its host's first part.
    let out = m.cli(&["logout", "b-team"], "");
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains(&format!("Signed out of {}", m.name(1))), "{}", text(&out));
    assert!(!m.file().contains(HOSTS[1]) && m.file().contains(HOSTS[0]), "{}", m.file());
    // A's part of the cache is still there: with the network off it is drawn from it.
    let before = m.methods(0).len();
    let Some(mut run) = m.start(2, (100, 30), &[("SLAKIO_NETWORK", "off")]) else { return };
    run.wait_for("A from the cache", |t| t.contains(&a) && t.contains("cached"));
    run.type_bytes(b" W");
    run.wait_for("the switcher", |t| t.contains(&m.name(0)) && t.contains("Esc"));
    assert!(!run.output().contains(&m.name(1)), "B is not listed: {}", run.output());
    run.type_bytes(b"\x1b");
    std::thread::sleep(Duration::from_millis(300));
    run.quit();
    assert_eq!(m.methods(0).len(), before, "nothing was asked");
    // The last one: everything goes.
    let out = m.cli(&["logout", "--all"], "");
    assert!(out.status.success(), "{}", text(&out));
    assert!(!m.workspaces_file().exists());
}

#[test]
fn each_workspaces_list_comes_back_from_the_cache_with_the_network_off() {
    let m = Machine::new("cache-two", &[0, 1]);
    let Some(run) = m.start(1, (100, 30), &[]) else { return };
    let (a, b) = (m.channel(0), m.channel(1));
    run.wait_for("A's channels", |t| t.contains(&a));
    wait_until("B booted", || m.methods(1).iter().any(|x| x == "client.userBoot"));
    // B's answer is kept before quitting.
    std::thread::sleep(Duration::from_millis(500));
    run.quit();
    let asked = (m.methods(0).len(), m.methods(1).len());
    let Some(mut run) = m.start(2, (100, 30), &[("SLAKIO_NETWORK", "off")]) else { return };
    run.wait_for("A from the cache", |t| t.contains(&a) && t.contains("cached"));
    run.type_bytes(b" Wj\r");
    run.wait_for("B from the cache", |t| t.contains(&b) && t.contains("cached"));
    run.quit();
    assert_eq!((m.methods(0).len(), m.methods(1).len()), asked, "the network off: nothing asked of either");
}

#[test]
fn signing_in_from_the_app_adds_the_workspace_and_keeps_the_one_signed_in_to() {
    let m = Machine::new("app-login", &[0]);
    let (creds, addrs) = m.env();
    let env = [("SLAKIO_TEST_CREDENTIALS", creds.as_str()), ("SLAKIO_TEST_SLACK_ADDR", addrs.as_str())];
    let config = m.dir.join("config-1.toml");
    std::fs::copy(m.dir.join("config.toml"), &config).unwrap();
    let Some(mut run) = Run::spawn_with(&m.dir, &config, &m.dir.join("typescript-1"), &env, (120, 40), "login") else {
        return;
    };
    run.wait_for("the paste screen", |t| t.contains("Waiting for the paste"));
    let paste = format!(
        "curl 'https://{}/api/client.counts' -H 'cookie: b=1; d={COOKIE}' --data-raw 'token={TOKEN}'",
        HOSTS[1]
    );
    run.type_bytes(format!("\x1b[200~{paste}\x1b[201~").as_bytes());
    run.wait_for("signed in to B", |t| t.contains(&format!("Signed in to {}", m.name(1))));
    // Both are listed and booted: A, signed in to before, first.
    run.wait_for("A's channels", |t| t.contains(&m.channel(0)));
    wait_until("B booted", || m.methods(1).iter().filter(|x| *x == "client.userBoot").count() >= 2);
    run.type_bytes(b" Wj\r");
    run.wait_for("B's channels", |t| t.contains(&m.channel(1)));
    run.quit();
    let file = m.file();
    assert!(file.contains(HOSTS[0]) && file.contains(HOSTS[1]), "{file}");
    assert!(!file.contains(TOKEN) && !file.contains(COOKIE));
}

#[test]
fn signing_out_of_one_workspace_while_a_slakio_runs_changes_nothing_and_says_to_quit_it() {
    let m = Machine::new("logout-running", &[0, 1]);
    let Some(run) = m.start(1, (100, 30), &[]) else { return };
    run.wait_for("A's channels", |t| t.contains(&m.channel(0)));
    let file = m.file();
    let out = m.cli(&["logout", "b-team"], "");
    assert!(!out.status.success());
    assert!(text(&out).contains("A slakio is still running"), "{}", text(&out));
    assert_eq!(m.file(), file, "still signed in to both: nothing is left half done");
    run.quit();
    // Quit: now it goes.
    let out = m.cli(&["logout", "b-team"], "");
    assert!(out.status.success(), "{}", text(&out));
    assert!(!m.file().contains(HOSTS[1]));
}
