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
        let ws = &self.world.snapshot().workspaces[0];
        let creds = format!("{}|{TOKEN}|{COOKIE}", ws.id);
        let addr = self.fake.addr().to_string();
        let env = [
            ("SLAKIO_TEST_CREDENTIALS", creds.as_str()),
            ("SLAKIO_TEST_SLACK_ADDR", addr.as_str()),
            ("SLAKIO_IMAGES", "on"),
            ("TERM", "xterm-kitty"),
        ];
        Run::spawn(&self.dir, &self.dir.join("config.toml"), &self.dir.join("typescript"), &env, (120, 14))
    }

    /// Each 1:1 DM of the first workspace whose peer has a photo: its name and the photo's path.
    fn dm_photos(&self) -> Vec<(String, String)> {
        let s = self.world.snapshot();
        let ws = &s.workspaces[0].id;
        (s.conversations.iter().filter(|c| c.workspace == *ws))
            .filter_map(|c| {
                let ConversationKind::Dm { user } = &c.kind else { return None };
                let u = s.users.iter().find(|u| u.id == *user && u.workspace == *ws)?;
                let path =
                    u.avatar.as_ref()?.strip_prefix("https://")?.split_once('/').map(|(_, p)| format!("/{p}"))?;
                Some((c.name.unsanitized().to_string(), path))
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
    // The DMs' people as the edge cache answers: each with a photo address, all but the first
    // marked as having no photo of their own (Slack's default only). The first, with a photo of
    // their own, is the control: it shows photos are fetched ahead at all.
    let control = dms[0].1.clone();
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
    // The others would have been asked with it, ahead in the same queue; a moment more for any late.
    std::thread::sleep(Duration::from_secs(1));
    let per = m.photo_fetches();
    run.quit();
    let fetched: Vec<&String> =
        per.keys().filter(|p| **p != control && dms.iter().any(|(_, path)| path == *p)).collect();
    assert!(fetched.is_empty(), "no photo of a DM peer without one of their own is fetched: {fetched:?}");
}
