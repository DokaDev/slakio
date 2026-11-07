//! Searching and reading from a day with the real binary in a pseudo terminal, against the
//! tests' fake Slack, at 120x40 and 80x24, in English and Korean: `/`, words, `Enter` sends one
//! `search.messages` (and name lookups), the results show; `Enter` on one opens its conversation
//! read around it (`conversations.history` only); `:jump <day>` reads it from that day (a few
//! `conversations.history` requests). Nothing else goes to Slack. Unix only.
//!
//! When `script` cannot start, each test prints a visible `SKIPPED` line and passes; with
//! `SLAKIO_REQUIRE_PTY=1` (CI on Linux and macOS) it fails instead.
#![cfg(unix)]

#[path = "support/pty.rs"]
mod pty;

use pty::{COOKIE, Run, TOKEN};
use slakio_core::i18n::{I18n, Lang, Msg};
use slakio_core::model::ConversationId;
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
    fn new(name: &str, lang: &str) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let world = World::new(7);
        let fake = rt.block_on(FakeSlack::start(World::new(7))).unwrap();
        let dir = std::env::temp_dir().join(format!("slakio-search-{}-{name}", std::process::id()));
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

    fn start(&self, size: (u16, u16)) -> Option<Run> {
        let ws = &self.world.snapshot().workspaces[0];
        let creds = format!("{}|{TOKEN}|{COOKIE}", ws.id);
        let addr = self.fake.addr().to_string();
        let env = [
            ("SLAKIO_TEST_CREDENTIALS", creds.as_str()),
            ("SLAKIO_TEST_SLACK_ADDR", addr.as_str()),
            ("SLAKIO_TEST_OPENER", "/nonexistent-opener"),
        ];
        Run::spawn(&self.dir, &self.dir.join("config.toml"), &self.dir.join("typescript"), &env, size)
    }

    fn calls(&self, method: &str) -> usize {
        self.fake.methods().iter().filter(|m| *m == method).count()
    }

    /// The methods asked for since `from` (an index into the log).
    fn since(&self, from: usize) -> Vec<String> {
        self.fake.methods()[from..].to_vec()
    }
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn has(t: &str, w: &str) -> bool {
    t.replace(' ', "").contains(&w.replace(' ', ""))
}

#[test]
fn a_search_finds_and_opens_a_message_and_a_day_reads_from_its_first() {
    for (size, lang) in [((120, 40), Lang::En), ((80, 24), Lang::En), ((120, 40), Lang::Ko), ((80, 24), Lang::Ko)] {
        let code = if lang == Lang::Ko { "ko" } else { "en" };
        let m = Machine::new(&format!("{}-{code}", size.0), code);
        let Some(mut run) = m.start(size) else { return };
        let i18n = I18n::new(lang);
        run.wait_for("the list", |t| t.contains("deploys"));
        wait_until("the start", || m.calls("client.userBoot") >= 1);
        std::thread::sleep(Duration::from_millis(300));
        let asked = m.fake.log().len();
        run.type_bytes(b"/");
        let title = i18n.msg(&Msg::SearchTitle { workspace: "example.slack.com".into() }).to_string();
        run.wait_for("the search popup", |t| has(t, &title));
        let query = format!("deploy in:#{}", names::BIG_HISTORY);
        run.type_bytes(query.as_bytes());
        run.type_bytes(b"\r");
        let ws = m.world.snapshot().workspaces[0].id.clone();
        let all = m.world.search(&ws, &query);
        let pages = all.len().div_ceil(20);
        let count = i18n
            .msg(&Msg::SearchCount { total: all.len().to_string(), page: "1".into(), pages: pages.to_string() })
            .to_string();
        run.wait_for("the results", |t| has(t, &count));
        let methods = m.since(asked);
        assert_eq!(methods.iter().filter(|x| *x == "search.messages").count(), 1, "{size:?} {code}: {methods:?}");
        assert!(methods.iter().all(|x| x == "search.messages" || x == "users/info"), "{methods:?}");
        // The newest result opens its conversation, read around it.
        let asked = m.fake.log().len();
        run.type_bytes(b"\r");
        run.wait_for("the conversation", |t| t.contains("big-history") && !has(t, &title));
        wait_until("the read around", || m.calls("conversations.history") >= 2);
        std::thread::sleep(Duration::from_millis(300));
        let methods = m.since(asked);
        assert!(methods.iter().all(|x| x == "conversations.history" || x == "users/info"), "{methods:?}");
        // A day: read from its first message, a few requests.
        let big = ConversationId::new(m.world.find(ws.as_str(), names::BIG_HISTORY).unwrap().id.as_str());
        let day = slakio_tui::time::date(m.world.message(&big, 4000).unwrap().ts);
        let asked = m.fake.log().len();
        run.type_bytes(format!(":jump {day}\r").as_bytes());
        run.wait_for("the day", |t| t.contains(&day));
        std::thread::sleep(Duration::from_millis(300));
        let methods = m.since(asked);
        let reads = methods.iter().filter(|x| *x == "conversations.history").count();
        assert!((2..=7).contains(&reads), "{size:?} {code}: {methods:?}");
        assert!(methods.iter().all(|x| x == "conversations.history" || x == "users/info"), "{methods:?}");
        run.quit();
    }
}
