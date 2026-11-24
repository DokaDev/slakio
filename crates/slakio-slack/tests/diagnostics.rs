//! The picture diagnostics against the fake Slack: off by default (no file, no sidebar probe);
//! on, a line for each stage of a photo, a copy, an emoji, the emoji list, the user's own photo,
//! the start bundle's shape and the sidebar's sections, and none of the fixture's tokens,
//! addresses, paths, names, ids or text in the file. The sidebar probe is read-only, counted by
//! the governor, and stops after its page cap.

use serde_json::json;
use slakio_core::backend::{BackendHandle, Command, Envelope, Event, Generation, Request};
use slakio_core::diag::Diag;
use slakio_core::images::emoji::Custom;
use slakio_core::images::{AVATAR_HOSTS, BYTES_MAX, Source};
use slakio_core::model::Target;
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault, PICTURES};
use slakio_slack::Client;
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_world::World;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const TOKEN: &str = concat!("xo", "xc-TEST");
const COOKIE: &str = "cookie-TEST";
const SECTIONS: &str = "users.channelSections.list";

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-diag-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("state").join("pictures.log")
}

struct Run {
    fake: FakeSlack,
    handle: BackendHandle,
    governor: Arc<Governor>,
    world: World,
    id: u64,
}

impl Run {
    async fn new(diag: Diag) -> Self {
        let fake = FakeSlack::start(World::demo()).await.unwrap();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client =
            Client::new(Credentials::new(TOKEN, COOKIE), Endpoints::local(fake.addr()).unwrap(), Arc::clone(&governor))
                .with_diag(diag);
        Self { fake, handle: slakio_slack::serve(client), governor, world: World::demo(), id: 0 }
    }

    async fn ask(&mut self, command: Command) -> Event {
        self.id += 1;
        self.handle.send(Request { id: Generation(self.id), workspace: None, command }).unwrap();
        let e: Envelope =
            tokio::time::timeout(Duration::from_secs(20), self.handle.rx.recv()).await.expect("an answer").unwrap();
        assert_eq!(e.generation, Generation(self.id));
        e.event
    }

    fn calls(&self, method: &str) -> usize {
        self.fake.log().iter().filter(|l| l.method() == method).count()
    }

    /// Wait (a few seconds at most) until `method` was asked `n` times.
    async fn until(&self, method: &str, n: usize) {
        for _ in 0..200 {
            if self.calls(method) >= n {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("{method} asked {} times, not {n}", self.calls(method));
    }

    fn target(&self, name: &str) -> Target {
        let s = self.world.snapshot();
        let c = s.conversations.iter().find(|c| c.workspace == s.workspaces[0].id && c.name == name).unwrap();
        Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() }
    }

    /// Everything of the fixture the file may never hold: the sign-in, every address and each
    /// part of its path, every name, id and text of the world.
    fn secrets(&self, extra: &[String]) -> Vec<String> {
        let s = self.world.snapshot();
        let mut out = vec![TOKEN.to_string(), COOKIE.to_string(), "TEST".to_string()];
        let address = |a: &str, out: &mut Vec<String>| {
            out.push(a.to_string());
            let path = a.split_once("://").map_or(a, |(_, r)| r).split_once('/').map_or("", |(_, p)| p);
            out.extend(path.split(['/', '?', '&', '=', '.']).map(str::to_string));
        };
        for w in &s.workspaces {
            out.extend([w.id.as_str().to_string(), w.name.unsanitized().to_string(), w.me.as_str().to_string()]);
        }
        for u in &s.users {
            out.extend([u.id.as_str().to_string(), u.name.unsanitized().to_string()]);
            out.push(u.display_name.unsanitized().to_string());
            if let Some(a) = &u.avatar {
                address(a, &mut out);
            }
        }
        for c in &s.conversations {
            out.extend([c.id.as_str().to_string(), c.name.unsanitized().to_string()]);
            for i in 0..self.world.message_count(&c.id) {
                let m = self.world.message(&c.id, i).unwrap();
                out.push(m.text.unsanitized().to_string());
                for f in &m.files {
                    out.push(f.name.unsanitized().to_string());
                    out.push(f.id.as_str().to_string());
                    for a in [&f.thumb, &f.download].into_iter().flatten() {
                        address(a, &mut out);
                    }
                }
            }
        }
        for (name, value) in slakio_world::emoji::list(s.workspaces[0].id.as_str()) {
            out.push(name);
            address(&value, &mut out);
        }
        for e in extra {
            address(e, &mut out);
        }
        out.extend(["LFAKE", "fake-page-2", "fake-ids-cursor"].map(str::to_string));
        // A part this short (`72`, `png`, `com`) says nothing; the hosts may be told.
        let hosts =
            [AVATAR_HOSTS.as_slice(), &slakio_core::images::EMOJI_HOSTS, &[slakio_core::model::FILE_HOST, "i0.wp.com"]]
                .concat();
        out.retain(|s| s.chars().count() >= 4 && !hosts.iter().any(|h| h.contains(s.as_str())));
        out.sort();
        out.dedup();
        out
    }
}

/// A photo on Gravatar whose fallback is Slack's default avatar (on the path the fake serves
/// pictures on).
fn gravatar() -> String {
    format!(
        "https://{}/avatars/0123456789abcdef0123456789abcdef.jpg?s=72&d=https%3A%2F%2F{}%2Favatars%2Fava_0001-72.png",
        AVATAR_HOSTS[1], AVATAR_HOSTS[2]
    )
}

#[test]
fn off_by_default_no_file_and_no_sidebar_probe() {
    rt().block_on(async {
        let path = scratch("off");
        let diag = Diag::from_env(|_| None, Some(path.clone()));
        let mut r = Run::new(diag.clone()).await;
        assert!(matches!(r.ask(Command::Boot).await, Event::Booted(_)));
        let target = r.target("design-review");
        assert!(matches!(r.ask(Command::History { target, before: None, limit: 20 }).await, Event::History(_)));
        assert!(matches!(r.ask(Command::Emoji).await, Event::Emoji { .. }));
        tokio::time::sleep(Duration::from_millis(300)).await;
        diag.flush();
        assert_eq!(r.calls(SECTIONS), 0, "no probe");
        assert!(!path.parent().unwrap().exists(), "no file, no folder");
    });
}

#[test]
fn on_every_stage_has_its_line_and_the_file_holds_nothing_of_the_fixture() {
    rt().block_on(async {
        let path = scratch("on");
        let diag = Diag::to(path.clone());
        let mut r = Run::new(diag.clone()).await;
        assert!(matches!(r.ask(Command::Boot).await, Event::Booted(_)));
        r.until(SECTIONS, 2).await;
        let target = r.target("design-review");
        let Event::History(page) = r.ask(Command::History { target, before: None, limit: 20 }).await else { panic!() };
        let Event::Emoji { emoji } = r.ask(Command::Emoji).await else { panic!() };
        let emoji_picture = emoji.iter().find_map(|(_, c)| match c {
            Custom::Picture(a) => Some(a.clone()),
            Custom::Standard(_) => None,
        });
        let photo = page.users.iter().find_map(|u| u.avatar.clone()).expect("a photo");
        let thumb = page.messages.iter().flat_map(|m| &m.files).find_map(|f| f.thumb.clone()).expect("a copy");
        for source in [Source::Avatar(photo), Source::Thumb(thumb), Source::Emoji(emoji_picture.expect("an emoji"))] {
            assert!(matches!(r.ask(Command::Image { source, max: BYTES_MAX }).await, Event::Image { .. }));
        }
        let cdn = format!("https://i0.wp.com/{}/img/ava_0001-72.png?ssl=1", AVATAR_HOSTS[2]);
        let to_cdn = vec![("location".to_string(), cdn.clone())];
        r.fake.fail_next(PICTURES, Fault::Raw { status: 302, headers: to_cdn, body: Vec::new() });
        // Gravatar sending elsewhere: its Slack fallback is asked instead, and draws.
        let fallen = r.ask(Command::Image { source: Source::Avatar(gravatar()), max: BYTES_MAX }).await;
        assert!(matches!(fallen, Event::Image { .. }), "{fallen:?}");
        diag.flush();
        let text = std::fs::read_to_string(&path).unwrap();
        for line in [
            "avatar asked ok",
            "avatar status 200",
            "avatar content-type image/png",
            "avatar parsed ok",
            "thumb asked ok",
            "emoji asked ok",
            "emoji content-type image/png",
            "emoji-list asked ok after_boot=yes",
            "emoji-list status 200",
            "emoji-list api ok",
            "emoji-list counted names",
            "emoji-list counted resolved",
            "self-photo parsed bundle read=no",
            "self-photo source",
            "self-photo parsed profile present=no",
            "boot-shape field present name=starred type=array count=1",
            "boot-shape field ims_key name=is_open",
            "sections asked ok page=1 cursor=no",
            "sections asked ok page=2 cursor=yes",
            "sections parsed section page=1 n=0 type=standard",
            "is_custom=yes ids=2 ids_page=yes ids_cursor=yes next_id=yes",
            "sections counted done pages=2",
            &format!("d_host={} d_allowed=yes", AVATAR_HOSTS[2]),
            "avatar status 302 id=",
            "avatar redirected refused id=",
            "avatar redirected fallback id=",
        ] {
            assert!(text.contains(line), "no `{line}` in\n{text}");
        }
        assert!(text.contains("host=i0.wp.com"), "{text}");
        let fallback = format!("hop=0 host={}", AVATAR_HOSTS[2]);
        assert!(text.lines().any(|l| l.contains("avatar redirected fallback") && l.contains(&fallback)), "{text}");
        let asked = format!("hop=1 host={}", AVATAR_HOSTS[2]);
        assert!(text.lines().any(|l| l.contains("avatar asked ok") && l.contains(&asked)), "{text}");
        let secrets = r.secrets(&[gravatar(), cdn]);
        assert!(secrets.len() > 100, "the fixture's names, ids, texts and paths: {}", secrets.len());
        for secret in secrets {
            assert!(!text.contains(&secret), "`{secret}` in\n{text}");
        }
        for line in text.lines() {
            let (ts, rest) = line.split_once(' ').unwrap();
            assert!(ts.ends_with('Z') && ts.len() == 24, "{line}");
            assert!(rest.split(' ').count() >= 3, "{line}");
        }
    });
}

#[test]
fn the_sidebar_probe_is_counted_and_stops_at_its_page_cap() {
    rt().block_on(async {
        let path = scratch("cap");
        let diag = Diag::to(path.clone());
        let mut r = Run::new(diag.clone()).await;
        let endless = json!({"ok": true, "channel_sections": [], "response_metadata": {"next_cursor": "more"}});
        for _ in 0..8 {
            r.fake.fail_next(SECTIONS, Fault::Answer(endless.clone()));
        }
        assert!(matches!(r.ask(Command::Boot).await, Event::Booted(_)));
        r.until(SECTIONS, 5).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        diag.flush();
        assert_eq!(r.calls(SECTIONS), 5, "five pages at most");
        let api = r.fake.log().iter().filter(|l| l.method() != PICTURES).count();
        assert_eq!(r.governor.sent() as usize, api, "every request counted, the probe's too");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("sections counted page-cap pages=5"), "{text}");
        // Booted again: the probe is not run twice.
        assert!(matches!(r.ask(Command::Boot).await, Event::Booted(_)));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(r.calls(SECTIONS), 5);
    });
}
