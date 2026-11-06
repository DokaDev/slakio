//! Slack links opened inside slakio with the real binary in a pseudo terminal, against the
//! tests' fake Slack, at 120x40 and 80x24, in English and Korean: `:open` with a link to an old
//! message of the workspace signed in to opens its conversation read around it (two
//! `conversations.history` requests, nothing more) and says the newer messages are not loaded;
//! a link to a workspace not signed in to asks first and, on `y`, goes to the opener (a stand-in
//! that records it; no browser starts). Unix only.
//!
//! When `script` cannot start, each test prints a visible `SKIPPED` line and passes; with
//! `SLAKIO_REQUIRE_PTY=1` (CI on Linux and macOS) it fails instead.
#![cfg(unix)]

#[path = "support/pty.rs"]
mod pty;

use pty::{COOKIE, Run, TOKEN};
use slakio_core::i18n::{I18n, Label, Lang, Msg};
use slakio_fake::FakeSlack;
use slakio_world::{World, names};
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
    /// A machine of its own for test `name`, signed in to `example.slack.com`, in `lang`.
    fn new(name: &str, lang: &str) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let world = World::new(7);
        let fake = rt.block_on(FakeSlack::start(World::new(7))).unwrap();
        let dir = std::env::temp_dir().join(format!("slakio-jump-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = dir.join("config").join("slakio");
        std::fs::create_dir_all(&config).unwrap();
        let ws = &world.snapshot().workspaces[0];
        let file = format!(
            "[[workspace]]\nteam = \"{}\"\nname = \"Fake\"\nhost = \"example.slack.com\"\nuser = \"{}\"\n",
            ws.id, ws.me
        );
        std::fs::write(config.join("workspaces.toml"), file).unwrap();
        let settings = format!("icons = \"off\"\ncache = \"off\"\nlive = \"off\"\nlanguage = \"{lang}\"\n");
        std::fs::write(dir.join("config.toml"), settings).unwrap();
        Self { _rt: rt, fake, world, dir }
    }

    fn start(&self, size: (u16, u16), opener: &str) -> Option<Run> {
        let ws = &self.world.snapshot().workspaces[0];
        let creds = format!("{}|{TOKEN}|{COOKIE}", ws.id);
        let addr = self.fake.addr().to_string();
        let env = [
            ("SLAKIO_TEST_CREDENTIALS", creds.as_str()),
            ("SLAKIO_TEST_SLACK_ADDR", addr.as_str()),
            ("SLAKIO_TEST_OPENER", opener),
        ];
        Run::spawn(&self.dir, &self.dir.join("config.toml"), &self.dir.join("typescript"), &env, size)
    }

    fn calls(&self, method: &str) -> usize {
        self.fake.methods().iter().filter(|m| *m == method).count()
    }

    fn channel(&self, name: &str) -> String {
        let s = self.world.snapshot();
        s.conversations.iter().find(|c| c.workspace == s.workspaces[0].id && c.name == name).unwrap().id.to_string()
    }

    /// A stand-in for the system's opener that records each argument it is handed.
    fn recorder(&self) -> (String, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let (opener, record) = (self.dir.join("opener.sh"), self.dir.join("opened.txt"));
        let script = format!("#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> '{}'; done\n", record.display());
        std::fs::write(&opener, script).unwrap();
        std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o700)).unwrap();
        (opener.to_string_lossy().into_owned(), record)
    }
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Blanks left out to compare (a wide character is drawn with one after it here).
fn has(t: &str, w: &str) -> bool {
    t.replace(' ', "").contains(&w.replace(' ', ""))
}

fn open(run: &mut Run, address: &str) {
    run.type_bytes(b":");
    run.type_bytes(format!("open {address}").as_bytes());
    run.type_bytes(b"\r");
}

#[test]
fn a_slack_link_opens_its_message_here_and_another_workspace_s_asks_for_the_browser() {
    for (size, lang) in [((120, 40), Lang::En), ((80, 24), Lang::En), ((120, 40), Lang::Ko), ((80, 24), Lang::Ko)] {
        let code = if lang == Lang::Ko { "ko" } else { "en" };
        let m = Machine::new(&format!("{}-{code}", size.0), code);
        let (opener, record) = m.recorder();
        let Some(mut run) = m.start(size, &opener) else { return };
        let i18n = I18n::new(lang);
        run.wait_for("the list", |t| t.contains("deploys"));
        wait_until("the start", || m.calls("client.userBoot") >= 1);
        let big = m.channel(names::BIG_HISTORY);
        let at = m.world.message(&slakio_core::model::ConversationId::new(&big), 1000).unwrap().ts;
        let asked = m.fake.log().len();
        open(&mut run, &format!("https://example.slack.com/archives/{big}/p{:016}", at.0));
        let cut = i18n.label(Label::PaneNewerCut).to_string();
        run.wait_for("the message read around", |t| has(t, &cut) && t.contains("big-history"));
        // Two reads around it, and its authors named (in batches); nothing else.
        let log = m.fake.log();
        let methods: Vec<&str> = log[asked..].iter().map(|l| l.method()).collect();
        let reads = methods.iter().filter(|m| **m == "conversations.history").count();
        assert_eq!(reads, 2, "{size:?} {code}: {methods:?}");
        assert!(methods.iter().all(|m| ["conversations.history", "users/info"].contains(m)), "{methods:?}");
        let asked = log.len();
        // Another workspace's link: asked first, the safe answer focused; `y` hands it over.
        let elsewhere = "https://elsewhere.slack.com/archives/C0NOTHERE/p1700000000000100";
        open(&mut run, elsewhere);
        let title = i18n.label(Label::DialogBrowserTitle).to_string();
        run.wait_for("the question", |t| has(t, &title));
        let why = i18n.msg(&Msg::JumpElsewhere { host: "elsewhere.slack.com".into() }).to_string();
        assert!(has(&run.output(), &why.chars().take(16).collect::<String>()), "{size:?} {code}: {}", run.output());
        run.type_bytes(b"y");
        wait_until("the browser link", || std::fs::read_to_string(&record).is_ok_and(|r| r.lines().count() == 1));
        assert_eq!(std::fs::read_to_string(&record).unwrap().trim(), elsewhere);
        assert_eq!(m.fake.log().len(), asked, "nothing more went to Slack: {:?}", m.fake.methods());
        run.quit();
    }
}
