//! Activity with the real binary in a pseudo terminal, against the tests' fake Slack, at 120x40
//! and 80x24, in English and Korean, times in a pinned zone (`SLAKIO_TZ`): `Space a` reads its
//! newest page (one `activity.feed`, a few `messages.list`, name lookups) and lists it; nothing is
//! read for it before it shows; `Enter` opens the newest notice's conversation read around its
//! message (`conversations.history` only); a mention that comes live over the socket is listed
//! at once, without a request. Nothing else goes to Slack, nothing is marked read. Unix only.
//!
//! When `script` cannot start, each test prints a visible `SKIPPED` line and passes; with
//! `SLAKIO_REQUIRE_PTY=1` (CI on Linux and macOS) it fails instead.
#![cfg(unix)]

#[path = "support/pty.rs"]
mod pty;

use pty::{COOKIE, Run, TOKEN};
use serde_json::json;
use slakio_core::i18n::Lang;
use slakio_fake::FakeSlack;
use slakio_tui::time::{Zone, date, hm};
use slakio_world::World;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const ZONE: &str = "Asia/Seoul";

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
        let dir = std::env::temp_dir().join(format!("slakio-activity-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = dir.join("config").join("slakio");
        std::fs::create_dir_all(&config).unwrap();
        let ws = &world.snapshot().workspaces[0];
        let file = format!(
            "[[workspace]]\nteam = \"{}\"\nname = \"Fake\"\nhost = \"example.slack.com\"\nuser = \"{}\"\n",
            ws.id, ws.me
        );
        std::fs::write(config.join("workspaces.toml"), file).unwrap();
        let settings = format!("icons = \"off\"\ncache = \"off\"\nlanguage = \"{lang}\"\n");
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
            ("SLAKIO_TZ", ZONE),
        ];
        Run::spawn(&self.dir, &self.dir.join("config.toml"), &self.dir.join("typescript"), &env, size)
    }

    fn calls(&self, method: &str) -> usize {
        self.fake.methods().iter().filter(|m| *m == method).count()
    }

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

#[test]
fn activity_lists_the_newest_page_opens_a_notice_and_takes_a_live_mention() {
    for (size, lang) in [((120, 40), Lang::En), ((80, 24), Lang::En), ((120, 40), Lang::Ko), ((80, 24), Lang::Ko)] {
        let code = if lang == Lang::Ko { "ko" } else { "en" };
        let m = Machine::new(&format!("{}-{code}", size.0), code);
        let Some(mut run) = m.start(size) else { return };
        run.wait_for("the list", |t| t.contains("deploys"));
        wait_until("the live updates", || m.fake.open_sockets() >= 1);
        assert_eq!(m.calls("activity.feed"), 0, "nothing read for Activity before it shows");
        let ws = m.world.snapshot().workspaces[0].clone();
        let all = m.world.activity(&ws.id);
        let newest = &all[0];
        let place = m.world.snapshot().conversations.iter().find(|c| c.id == newest.conversation).unwrap();
        let place = format!("{}{}", if place.is_dm() { "@" } else { "#" }, place.name.unsanitized());
        let asked = m.fake.log().len();
        run.type_bytes(b" a");
        run.wait_for("the newest notice", |t| t.contains(&place));
        wait_until("its texts", || m.calls("messages.list") >= 1);
        std::thread::sleep(Duration::from_millis(300));
        let methods = m.since(asked);
        assert_eq!(methods.iter().filter(|x| *x == "activity.feed").count(), 1, "{size:?} {code}: {methods:?}");
        assert!(methods.iter().filter(|x| *x == "messages.list").count() <= 3, "{methods:?}");
        assert!(
            methods.iter().all(|x| ["activity.feed", "messages.list", "users/info"].contains(&x.as_str())),
            "{methods:?}"
        );
        // Its time in the zone pinned: the time alone today, else with its day.
        let zone = Zone::named(ZONE).unwrap();
        let (day, time) = (date(newest.at, &zone), hm(newest.at, &zone));
        let screen = run.output();
        assert!(
            screen.contains(&time) || screen.contains(&day) || screen.contains(&day[5..]),
            "{day} {time}: {screen}"
        );
        // The newest notice opens its conversation read around its message.
        let asked = m.fake.log().len();
        run.type_bytes(b"\r");
        wait_until("the read around", || {
            m.since(asked).iter().any(|x| x == "conversations.history" || x == "conversations.replies")
        });
        std::thread::sleep(Duration::from_millis(300));
        let methods = m.since(asked);
        assert!(
            methods
                .iter()
                .all(|x| ["conversations.history", "conversations.replies", "users/info"].contains(&x.as_str())),
            "{methods:?}"
        );
        // A mention that comes live: listed at once, nothing asked for it.
        run.type_bytes(b" a");
        let c = m.world.find(ws.id.as_str(), "deploys").unwrap().id.to_string();
        let who = m.world.snapshot().users.iter().find(|u| u.workspace == ws.id && u.id != ws.me && !u.bot).unwrap();
        let asked = m.fake.log().len();
        let text = format!("live ping <@{}>", ws.me);
        m.fake.push(
            &json!({"type": "message", "channel": c, "user": who.id.as_str(), "text": text, "ts": "1999000000.000100"}),
        );
        run.wait_for("the live mention", |t| t.contains("live ping"));
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(m.since(asked).iter().filter(|x| *x == "activity.feed").count(), 0, "not read again at once");
        run.quit();
    }
}
