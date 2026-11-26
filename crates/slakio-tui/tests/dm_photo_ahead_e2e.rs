//! Photos asked for ahead with the real binary in a pseudo terminal, against the tests' fake
//! Slack, pictures on (`SLAKIO_IMAGES=on`, kitty), `avatars = "image"` and the cache on disk on:
//! the DM rows the list leaves out of view get their peers' photos fetched ahead, each once,
//! within the share asked ahead in a minute, and kept on disk. Unix only.
//!
//! When `script` cannot start, each test prints a visible `SKIPPED` line and passes; with
//! `SLAKIO_REQUIRE_PTY=1` (CI on Linux and macOS) it fails instead.
#![cfg(unix)]

#[path = "support/pty.rs"]
mod pty;

use pty::{COOKIE, Run, TOKEN};
use slakio_core::model::ConversationKind;
use slakio_fake::{FakeSlack, PICTURES};
use slakio_tui::images::AHEAD_PER_MINUTE;
use slakio_world::World;
use std::collections::HashMap;
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
    fn new(name: &str) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let world = World::new(7);
        let fake = rt.block_on(FakeSlack::start(World::new(7))).unwrap();
        let dir = std::env::temp_dir().join(format!("slakio-ahead-e2e-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = dir.join("config").join("slakio");
        std::fs::create_dir_all(&config).unwrap();
        let ws = &world.snapshot().workspaces[0];
        let file = format!(
            "[[workspace]]\nteam = \"{}\"\nname = \"Fake\"\nhost = \"example.slack.com\"\nuser = \"{}\"\n",
            ws.id, ws.me
        );
        std::fs::write(config.join("workspaces.toml"), file).unwrap();
        let settings = "icons = \"off\"\ncache = \"on\"\nlive = \"off\"\nlanguage = \"en\"\navatars = \"image\"\n";
        std::fs::write(dir.join("config.toml"), settings).unwrap();
        Self { _rt: rt, fake, world, dir }
    }

    fn start(&self) -> Option<Run> {
        self.start_with((120, 14), "on")
    }

    /// [`Machine::start`] in a terminal of `size`, pictures `images`.
    fn start_with(&self, size: (u16, u16), images: &str) -> Option<Run> {
        let ws = &self.world.snapshot().workspaces[0];
        let creds = format!("{}|{TOKEN}|{COOKIE}", ws.id);
        let addr = self.fake.addr().to_string();
        let env = [
            ("SLAKIO_TEST_CREDENTIALS", creds.as_str()),
            ("SLAKIO_TEST_SLACK_ADDR", addr.as_str()),
            ("SLAKIO_IMAGES", images),
            ("TERM", "xterm-kitty"),
        ];
        Run::spawn(&self.dir, &self.dir.join("config.toml"), &self.dir.join("typescript"), &env, size)
    }

    /// The photo paths of [`Machine::dm_photos`] in the order Home lists their rows, top down
    /// (the order their photos are asked for ahead): read from a run of its own, its list on its
    /// last row in a terminal tall enough for every DM row above it, pictures off (nothing
    /// fetched). `None` when `script` cannot start.
    fn home_order(name: &str) -> Option<Vec<String>> {
        let m = Machine::new(name);
        let dms = m.dm_photos();
        let mut run = m.start_with((120, 90), "off")?;
        run.wait_for("the list", |t| t.contains("# backend"));
        run.type_bytes(b"G");
        // A row's text without its blanks (a wide letter is followed by one on this screen); a
        // DM's row is its name, then `you` on the user's own, its peer's status emoji and a count
        // (a group DM's goes on with a comma).
        let rows = |t: &str| -> Vec<String> {
            t.lines().map(|l| l.split('\u{2502}').nth(1).unwrap_or("").split_whitespace().collect()).collect()
        };
        let at = |rows: &[String], name: &str| {
            let name: String = name.split_whitespace().collect();
            rows.iter().position(|r| {
                r.split_once(name.as_str()).is_some_and(|(_, rest)| {
                    let rest = rest.strip_prefix("you").unwrap_or(rest);
                    rest.chars().all(|c| c.is_ascii_digit() || !c.is_ascii())
                })
            })
        };
        run.wait_for("every DM row", |t| dms.iter().all(|(n, _)| at(&rows(t), n).is_some()));
        let rows = rows(&run.output());
        run.quit();
        let mut order: Vec<(usize, String)> = dms.into_iter().map(|(n, p)| (at(&rows, &n).unwrap(), p)).collect();
        order.sort();
        Some(order.into_iter().map(|(_, p)| p).collect())
    }

    /// Each 1:1 DM of the first workspace whose peer has a photo: its name (as shown) and the
    /// photo's path.
    fn dm_photos(&self) -> Vec<(String, String)> {
        let s = self.world.snapshot();
        let ws = &s.workspaces[0].id;
        (s.conversations.iter().filter(|c| c.workspace == *ws))
            .filter_map(|c| {
                let ConversationKind::Dm { user } = &c.kind else { return None };
                let u = s.users.iter().find(|u| u.id == *user && u.workspace == *ws)?;
                let path =
                    u.avatar.as_ref()?.strip_prefix("https://")?.split_once('/').map(|(_, p)| format!("/{p}"))?;
                Some((c.name.line().as_str().to_string(), path))
            })
            .collect()
    }

    /// How many times each photo path was fetched.
    fn photo_fetches(&self) -> HashMap<String, usize> {
        let mut per = HashMap::new();
        for l in self
            .fake
            .log()
            .into_iter()
            .filter(|l| l.method() == PICTURES && slakio_world::pictures::is_avatar_path(&l.path))
        {
            *per.entry(l.path.clone()).or_default() += 1;
        }
        per
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
fn dm_rows_out_of_view_get_their_photos_ahead() {
    let m = Machine::new("dms");
    let dms = m.dm_photos();
    let Some(run) = m.start() else { return };
    let started = Instant::now();
    wait_until("the start", || m.fake.methods().iter().any(|x| x == "client.userBoot"));
    // Home: its channels fill the list's rows, its DMs are out of view; no conversation open.
    run.wait_for("the list", |t| t.contains("# backend"));
    // A DM out of view: its name is nowhere on screen.
    let out_of_view = |screen: &str| -> Vec<String> {
        dms.iter().filter(|(name, _)| !screen.contains(name.as_str())).map(|(_, path)| path.clone()).collect()
    };
    let hidden = out_of_view(&run.output());
    assert!(!hidden.is_empty(), "some DM rows out of view:\n{}", run.output());
    wait_until("a photo of a DM row out of view fetched ahead", || {
        m.photo_fetches().keys().any(|p| hidden.contains(p))
    });
    std::thread::sleep(Duration::from_secs(3));
    let elapsed = started.elapsed();
    let per = m.photo_fetches();
    run.quit();
    let ahead = per.keys().filter(|p| hidden.contains(p)).count();
    assert_eq!(ahead, per.len(), "no photo on screen, so every photo fetched is one ahead: {per:?}");
    let minutes = usize::try_from(elapsed.as_secs() / 60).unwrap() + 1;
    assert!(ahead >= 1 && ahead <= AHEAD_PER_MINUTE * minutes, "{ahead} fetched ahead in {elapsed:?}");
    assert!(per.values().all(|n| *n == 1), "each fetched once: {per:?}");
    let kept = m.dir.join("cache").join("slakio").join("pictures");
    let files = std::fs::read_dir(&kept).map_or(0, |d| d.count());
    assert!(files >= ahead, "the photos fetched ahead are kept on disk: {files} < {ahead}");
}

#[test]
fn a_dm_peer_without_a_photo_of_their_own_gets_no_photo_fetched() {
    let m = Machine::new("not-custom");
    let dms = m.dm_photos();
    assert!(dms.len() >= 2, "some DM peers with a photo");
    // The DMs' people as the edge cache answers: each with a photo address, all but one marked
    // as having no photo of their own (Slack's default only). That one, with a photo of their
    // own, is the control: it shows photos are fetched ahead at all. It is the row Home lists
    // last, so its photo is asked for ahead after any other's (one at a time, in the rows'
    // order): once it is fetched, none of the others is still to come.
    let Some(order) = Machine::home_order("not-custom-order") else { return };
    let control = order.last().expect("a DM row with a photo").clone();
    let s = m.world.snapshot();
    let ws = &s.workspaces[0].id;
    let results: Vec<serde_json::Value> = (s.conversations.iter().filter(|c| c.workspace == *ws))
        .filter_map(|c| match &c.kind {
            ConversationKind::Dm { user } => s.users.iter().find(|u| u.id == *user && u.workspace == *ws),
            _ => None,
        })
        .map(|u| {
            let own = u.avatar.as_ref().is_some_and(|a| a.ends_with(&control));
            serde_json::json!({"id": u.id.as_str(), "team_id": ws.as_str(), "name": u.name.unsanitized(),
                "profile": {"real_name": u.display_name.unsanitized(), "image_72": u.avatar, "is_custom_image": own}})
        })
        .collect();
    m.fake.fail_next("users/info", slakio_fake::Fault::Answer(serde_json::json!({"ok": true, "results": results})));
    let Some(run) = m.start() else { return };
    wait_until("the start", || m.fake.methods().iter().any(|x| x == "client.userBoot"));
    run.wait_for("the list", |t| t.contains("# backend"));
    wait_until("the DMs' people looked up", || m.fake.methods().iter().any(|x| x == "users/info"));
    wait_until("the control's photo fetched ahead", || m.photo_fetches().contains_key(&control));
    let per = m.photo_fetches();
    run.quit();
    let fetched: Vec<&String> =
        per.keys().filter(|p| **p != control && dms.iter().any(|(_, path)| path == *p)).collect();
    assert!(fetched.is_empty(), "no photo of a DM peer without one of their own is fetched: {fetched:?}");
}
