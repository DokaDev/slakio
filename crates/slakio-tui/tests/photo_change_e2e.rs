//! A profile photo changed live, with the real binary in a pseudo terminal against the tests'
//! fake Slack, pictures on (`SLAKIO_IMAGES=on`, kitty) and `avatars = "image"`: a `user_change`
//! with a new photo gets the new one fetched (its 72-pixel copy, nothing else asked of Slack)
//! and the old picture deleted from the terminal by its id; a later one that says no photo of
//! their own deletes that one too and fetches nothing. Unix only.
//!
//! When `script` cannot start, each test prints a visible `SKIPPED` line and passes; with
//! `SLAKIO_REQUIRE_PTY=1` (CI on Linux and macOS) it fails instead.
#![cfg(unix)]

#[path = "support/pty.rs"]
mod pty;

use pty::{COOKIE, Run, TOKEN};
use serde_json::json;
use slakio_core::model::ConversationKind;
use slakio_fake::{FakeSlack, PICTURES};
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
    fn new(name: &str) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let world = World::new(7);
        let fake = rt.block_on(FakeSlack::start(World::new(7))).unwrap();
        let dir = std::env::temp_dir().join(format!("slakio-photo-change-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = dir.join("config").join("slakio");
        std::fs::create_dir_all(&config).unwrap();
        let ws = &world.snapshot().workspaces[0];
        let file = format!(
            "[[workspace]]\nteam = \"{}\"\nname = \"Fake\"\nhost = \"example.slack.com\"\nuser = \"{}\"\n",
            ws.id, ws.me
        );
        std::fs::write(config.join("workspaces.toml"), file).unwrap();
        let settings = "icons = \"off\"\ncache = \"off\"\nlanguage = \"en\"\navatars = \"image\"\n";
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
        Run::spawn(&self.dir, &self.dir.join("config.toml"), &self.dir.join("typescript"), &env, (120, 40))
    }

    /// What the binary wrote so far.
    fn written(&self) -> Vec<u8> {
        std::fs::read(self.dir.join("typescript")).unwrap_or_default()
    }

    /// Each 1:1 DM of the first workspace whose peer has a photo: its name, the peer's id and
    /// shown name, and the photo's path.
    fn dm_photos(&self) -> Vec<(String, String, String, String)> {
        let s = self.world.snapshot();
        let ws = &s.workspaces[0].id;
        (s.conversations.iter().filter(|c| c.workspace == *ws))
            .filter_map(|c| {
                let ConversationKind::Dm { user } = &c.kind else { return None };
                let u = s.users.iter().find(|u| u.id == *user && u.workspace == *ws)?;
                let path =
                    u.avatar.as_ref()?.strip_prefix("https://")?.split_once('/').map(|(_, p)| format!("/{p}"))?;
                let shown = u.display_name.unsanitized().to_string();
                Some((c.name.unsanitized().to_string(), u.id.to_string(), shown, path))
            })
            .collect()
    }

    fn fetched(&self, path: &str) -> bool {
        self.fake.log().iter().any(|l| l.method() == PICTURES && l.path == path)
    }

    fn api_calls(&self) -> usize {
        self.fake.log().iter().filter(|l| l.method() != PICTURES).count()
    }
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// The command that deletes picture `id` (by its id).
fn deleted(id: u32) -> Vec<u8> {
    format!("\x1b_Ga=d,d=I,i={id},").into_bytes()
}

#[test]
fn a_photo_changed_live_is_fetched_anew_and_the_old_picture_leaves_the_terminal() {
    let m = Machine::new("change");
    let dms = m.dm_photos();
    let Some(mut run) = m.start() else { return };
    run.wait_for("live updates on", |t| t.contains("Live updates on"));
    // The DMs view: its rows draw their peers' photos.
    run.type_bytes(b" d");
    run.wait_for("a DM row", |t| dms.iter().any(|d| t.contains(d.0.as_str())));
    let screen = run.output();
    let (name, who, shown, old) =
        dms.iter().find(|d| screen.contains(d.0.as_str())).expect("a DM row on screen").clone();
    let row = screen.lines().position(|l| l.contains(name.as_str())).unwrap();
    wait_until("the peer's photo fetched", || m.fetched(&old));
    // The picture the peer's row shows: its id, which a delete must name.
    wait_until("the peer's photo drawn", || run.pictures_on_row(row).len() == 1);
    let old_id = run.pictures_on_row(row)[0];
    std::thread::sleep(Duration::from_millis(500));
    let (mark, api) = (m.written().len(), m.api_calls());
    let ws = m.world.snapshot().workspaces[0].id.to_string();
    let file = format!("{ws}-{who}-new");
    let original = format!("https://s3-us-west-2.amazonaws.com/slack-files2/avatars/2026-01-05/{file}_original.png");
    let profile = |p: serde_json::Value| json!({"id": who, "team_id": ws, "name": "peer", "profile": p});
    m.fake.push(&json!({"type": "user_change",
        "user": profile(json!({"display_name": shown, "image_original": original, "is_custom_image": true}))}));
    let new = format!("/2026-01-05/{file}_72.png");
    wait_until("the new photo fetched", || m.fetched(&new));
    wait_until("the old picture deleted", || contains(&m.written()[mark..], &deleted(old_id)));
    wait_until("the new photo drawn", || run.pictures_on_row(row).iter().any(|id| *id != old_id));
    let new_id = run.pictures_on_row(row).into_iter().find(|id| *id != old_id).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let mark = m.written().len();
    let fetches = m.fake.log().iter().filter(|l| l.method() == PICTURES).count();
    m.fake.push(&json!({"type": "user_profile_changed",
        "user": profile(json!({"display_name": shown, "image_original": original, "is_custom_image": false}))}));
    wait_until("the new picture deleted", || contains(&m.written()[mark..], &deleted(new_id)));
    std::thread::sleep(Duration::from_millis(500));
    let after = m.fake.log().iter().filter(|l| l.method() == PICTURES).count();
    run.quit();
    assert_eq!(after, fetches, "no photo fetched for no photo of their own");
    assert_eq!(m.api_calls(), api, "nothing asked of Slack for a profile change");
}
