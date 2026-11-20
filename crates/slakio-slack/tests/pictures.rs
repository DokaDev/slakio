//! Pictures against the fake Slack: a file's smaller copy fetched from the file host with the
//! session's cookie and never the token; a profile photo fetched from an avatar host with
//! neither; only what says it is an image read, at most the bytes asked; redirects followed
//! only on the file host; addresses elsewhere never asked; every ask a background request of the
//! budget; the switched-off network asks nothing. And their addresses read from Slack's
//! answers: only the file host for copies, only the avatar hosts for photos.

use serde_json::json;
use slakio_core::backend::{BackendHandle, Command, Envelope, Event, FailKind, Generation, Request};
use slakio_core::images::{AVATAR_HOSTS, BYTES_MAX, Source};
use slakio_core::model::{FILE_HOST, Target};
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault, PICTURES};
use slakio_slack::Client;
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_world::World;
use std::sync::Arc;
use std::time::Duration;

const TOKEN: &str = concat!("xo", "xc-TEST");

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

struct Run {
    fake: FakeSlack,
    handle: BackendHandle,
    governor: Arc<Governor>,
    world: World,
    id: u64,
}

impl Run {
    async fn with(limits: Limits) -> Self {
        let fake = FakeSlack::start(World::demo()).await.unwrap();
        let governor = Governor::new(limits, Box::new(FakeClock::default()), true);
        let client = Client::new(
            Credentials::new(TOKEN, "cookie-TEST"),
            Endpoints::local(fake.addr()).unwrap(),
            Arc::clone(&governor),
        );
        Self { fake, handle: slakio_slack::serve(client), governor, world: World::demo(), id: 0 }
    }

    async fn new() -> Self {
        Self::with(Limits::default()).await
    }

    async fn ask(&mut self, command: Command) -> Event {
        self.id += 1;
        self.handle.send(Request { id: Generation(self.id), workspace: None, command }).unwrap();
        let e: Envelope =
            tokio::time::timeout(Duration::from_secs(20), self.handle.rx.recv()).await.expect("an answer").unwrap();
        assert_eq!(e.generation, Generation(self.id));
        e.event
    }

    async fn picture(&mut self, source: Source, max: u64) -> Result<Vec<u8>, FailKind> {
        match self.ask(Command::Image { source, max }).await {
            Event::Image { bytes } => Ok(bytes),
            Event::Failed { kind, .. } => Err(kind),
            other => panic!("{other:?}"),
        }
    }

    /// The world's first file with a smaller copy, and a person's photo.
    fn sources(&self) -> (Source, Source) {
        let s = self.world.snapshot();
        let c = s.conversations.iter().find(|c| c.name == "design-review").unwrap();
        let thumb = (0..self.world.message_count(&c.id))
            .flat_map(|i| self.world.message(&c.id, i).unwrap().files)
            .find_map(|f| f.thumb)
            .expect("a picture file");
        let photo = s.users.iter().find_map(|u| u.avatar.clone()).expect("a photo");
        (Source::Thumb(thumb), Source::Avatar(photo))
    }

    fn picture_requests(&self) -> Vec<slakio_fake::Logged> {
        self.fake.log().into_iter().filter(|l| l.method() == PICTURES).collect()
    }

    fn target(&self, name: &str) -> Target {
        let s = self.world.snapshot();
        let c = s.conversations.iter().find(|c| c.workspace == s.workspaces[0].id && c.name == name).unwrap();
        Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() }
    }
}

fn header<'a>(l: &'a slakio_fake::Logged, name: &str) -> Option<&'a str> {
    l.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

fn api(code: &str) -> FailKind {
    FailKind::Api { code: code.to_string() }
}

fn png(content_type: &str, body: Vec<u8>) -> Fault {
    Fault::Raw { status: 200, headers: vec![("content-type".to_string(), content_type.to_string())], body }
}

#[test]
fn a_copy_goes_with_the_cookie_a_photo_with_nothing_of_the_session() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let (thumb, photo) = r.sources();
        let a = r.picture(thumb, BYTES_MAX).await.unwrap();
        let b = r.picture(photo, BYTES_MAX).await.unwrap();
        assert_eq!(&a[..4], b"\x89PNG");
        assert_eq!(&b[..4], b"\x89PNG");
        let log = r.picture_requests();
        assert_eq!(log.len(), 2);
        assert!(log[0].path.starts_with("/files-tmb/"));
        assert!(header(&log[0], "cookie").is_some_and(|c| c.contains("cookie-TEST")), "the copy: the cookie");
        assert!(log[1].path.starts_with("/avatars/"));
        assert_eq!(header(&log[1], "cookie"), None, "the photo: no cookie");
        assert_eq!(header(&log[1], "referer"), None);
        assert_eq!(header(&log[1], "origin"), None);
        for l in &log {
            assert_eq!(header(l, "authorization"), None);
            assert!(!l.query.contains("TEST") || !l.query.contains("xo"), "never the token");
            assert!(l.headers.iter().all(|(_, v)| !v.contains(TOKEN)), "never the token");
            assert!(l.body.is_empty(), "a GET");
        }
        assert_eq!(r.governor.sent(), 2, "each ask counts");
    });
}

#[test]
fn only_an_image_is_read_and_never_more_than_asked() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let (thumb, photo) = r.sources();
        r.fake.fail_next(PICTURES, png("text/html", b"<html>sign in</html>".to_vec()));
        assert_eq!(r.picture(thumb.clone(), BYTES_MAX).await, Err(api("not_an_image")));
        r.fake.fail_next(PICTURES, png("image/png", vec![0; 5000]));
        assert_eq!(r.picture(thumb.clone(), 4000).await, Err(FailKind::TooLarge), "said too large");
        let lying = Fault::Raw {
            status: 200,
            headers: vec![
                ("content-type".to_string(), "image/png".to_string()),
                ("transfer-encoding".to_string(), "chunked".to_string()),
            ],
            body: format!("{:x}\r\n{}\r\n0\r\n\r\n", 5000, "x".repeat(5000)).into_bytes(),
        };
        r.fake.fail_next(PICTURES, lying);
        assert_eq!(r.picture(photo.clone(), 4000).await, Err(FailKind::TooLarge), "counted too large");
        r.fake.fail_next(PICTURES, Fault::Status { status: 404, retry_after: None });
        assert_eq!(r.picture(photo.clone(), BYTES_MAX).await, Err(api("file_not_available")));
        r.fake.fail_next(PICTURES, Fault::Status { status: 429, retry_after: Some("7".to_string()) });
        assert_eq!(
            r.picture(photo, BYTES_MAX).await,
            Err(FailKind::RateLimited { retry_after: Some(Duration::from_secs(7)) })
        );
    });
}

#[test]
fn redirects_stay_on_the_file_host_and_a_photo_follows_none() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let (thumb, photo) = r.sources();
        let to = |at: &str| Fault::Raw {
            status: 302,
            headers: vec![("location".to_string(), at.to_string())],
            body: Vec::new(),
        };
        r.fake.fail_next(PICTURES, to("https://evil.example/x.png"));
        assert_eq!(r.picture(thumb.clone(), BYTES_MAX).await, Err(api("redirected_elsewhere")));
        r.fake.fail_next(PICTURES, to(&format!("https://{}/x.png", AVATAR_HOSTS[0])));
        assert_eq!(
            r.picture(thumb.clone(), BYTES_MAX).await,
            Err(api("redirected_elsewhere")),
            "not even an avatar host"
        );
        let path = thumb.address().split_once(".com").unwrap().1.to_string();
        r.fake.fail_next(PICTURES, to(&path));
        assert!(r.picture(thumb, BYTES_MAX).await.is_ok(), "on the file host: followed");
        r.fake.fail_next(PICTURES, to(photo.address()));
        assert_eq!(r.picture(photo, BYTES_MAX).await, Err(api("redirected_elsewhere")));
        let log = r.picture_requests();
        assert_eq!(log.len(), 5, "nothing asked elsewhere");
    });
}

#[test]
fn an_address_a_picture_may_not_come_from_is_never_asked() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let (thumb, photo) = r.sources();
        for source in [
            Source::Avatar(thumb.address().to_string()),
            Source::Thumb(photo.address().to_string()),
            Source::Avatar("https://evil.example/a.png".to_string()),
            Source::Thumb(format!("http://{FILE_HOST}/files-tmb/x.png")),
            Source::Avatar(format!("https://{}@evil.example/a.png", AVATAR_HOSTS[0])),
        ] {
            assert_eq!(r.picture(source.clone(), BYTES_MAX).await, Err(api("not_a_picture_address")), "{source:?}");
        }
        assert!(r.picture_requests().is_empty());
        assert_eq!(r.governor.sent(), 0);
    });
}

#[test]
fn pictures_are_background_requests_and_the_switch_stops_them() {
    rt().block_on(async {
        let limits = Limits { background_per_minute: 1, lane_background_per_minute: 1, ..Limits::default() };
        let mut r = Run::with(limits).await;
        let (thumb, photo) = r.sources();
        assert!(r.picture(thumb, BYTES_MAX).await.is_ok());
        assert!(matches!(r.picture(photo.clone(), BYTES_MAX).await, Err(FailKind::CapReached)), "the background share");
        r.governor.stop();
        assert!(matches!(r.picture(photo, BYTES_MAX).await, Err(FailKind::Offline)));
        assert_eq!(r.picture_requests().len(), 1);
    });
}

#[test]
fn copies_come_from_the_file_host_and_photos_from_the_avatar_hosts_only() {
    rt().block_on(async {
        let mut r = Run::new().await;
        // Signed in to its workspace: the people a page names are of it.
        assert!(matches!(r.ask(Command::Boot).await, Event::Booted(_)));
        let target = r.target("general");
        let ws = target.workspace().as_str().to_string();
        let good = format!("https://{FILE_HOST}/files-tmb/{ws}-F1/a_480.png");
        let answer = json!({"ok": true, "history": {"ok": true, "has_more": false, "messages": [
            {"type": "message", "user": "UPIC1", "ts": "1767603600.000100", "text": "",
             "files": [
                {"id": "F1", "name": "a.png", "filetype": "png", "mode": "hosted",
                 "url_private_download": format!("https://{FILE_HOST}/files-pri/{ws}-F1/download/a.png"),
                 "thumb_160": format!("https://{FILE_HOST}/files-tmb/{ws}-F1/a_160.png"),
                 "thumb_480": good, "thumb_720": "https://evil.example/a_720.png"},
                {"id": "F2", "name": "b.png", "filetype": "png", "mode": "hosted",
                 "url_private_download": format!("https://{FILE_HOST}/files-pri/{ws}-F2/download/b.png"),
                 "thumb_480": "https://evil.example/b.png", "thumb_360": format!("http://{FILE_HOST}/b.png")},
                {"id": "F3", "name": "c.pdf", "filetype": "pdf", "mode": "hosted",
                 "url_private_download": format!("https://{FILE_HOST}/files-pri/{ws}-F3/download/c.pdf"),
                 "thumb_480": format!("https://{FILE_HOST}/files-tmb/{ws}-F3/c.png")},
                {"id": "F4", "name": "d.png", "filetype": "png", "mode": "tombstone",
                 "thumb_480": format!("https://{FILE_HOST}/files-tmb/{ws}-F4/d.png")}
             ]},
            {"type": "message", "user": "UPIC2", "ts": "1767603601.000100", "text": "two"},
            {"type": "message", "user": "UPIC3", "ts": "1767603602.000100", "text": "three"}
        ]}, "users": [
            {"id": "UPIC1", "name": "pic", "profile": {"real_name": "Pic", "image_72": format!("https://{}/p/1_72.png", AVATAR_HOSTS[0])}},
            {"id": "UPIC2", "name": "evil", "profile": {"real_name": "Evil", "image_72": "https://evil.example/p.png",
             "image_48": format!("https://{}/p/2_48.png", AVATAR_HOSTS[1])}},
            {"id": "UPIC3", "name": "bad", "profile": {"real_name": "Bad", "image_72": format!("http://{}/p.png", AVATAR_HOSTS[0])}}
        ]});
        r.fake.fail_next("conversations.view", Fault::Answer(answer));
        let page = match r.ask(Command::History { target, before: None, limit: 28 }).await {
            Event::History(p) => *p,
            other => panic!("{other:?}"),
        };
        let files = &page.messages[0].files;
        assert_eq!(files[0].thumb.as_deref(), Some(good.as_str()), "the 480 one, on the file host");
        assert_eq!(files[1].thumb, None, "none on the file host over https");
        assert_eq!(files[2].thumb, None, "not a picture");
        assert_eq!(files[3].thumb, None, "deleted");
        let photo = |id: &str| page.users.iter().find(|u| u.id.as_str() == id).and_then(|u| u.avatar.clone());
        assert_eq!(photo("UPIC1"), Some(format!("https://{}/p/1_72.png", AVATAR_HOSTS[0])));
        assert_eq!(photo("UPIC2"), Some(format!("https://{}/p/2_48.png", AVATAR_HOSTS[1])), "the one on an avatar host");
        assert_eq!(photo("UPIC3"), None);
    });
}

#[test]
fn a_picture_that_trickles_ends_at_its_deadline() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let (thumb, _) = r.sources();
        r.fake.fail_next(PICTURES, Fault::Partial { length: 100_000, body: vec![b'x'; 100] });
        r.id += 1;
        let id = Generation(r.id);
        r.handle
            .send(Request { id, workspace: None, command: Command::Image { source: thumb, max: BYTES_MAX } })
            .unwrap();
        // Its first bytes came: then the clock runs by itself (paused, it jumps to each timer).
        for _ in 0..500 {
            if !r.picture_requests().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        tokio::time::pause();
        let e = r.handle.rx.recv().await.unwrap();
        assert_eq!(e.generation, id);
        match e.event {
            Event::Failed { kind, detail, .. } => {
                assert_eq!((kind, detail.as_str()), (FailKind::Network, "the picture took too long"));
            }
            other => panic!("{other:?}"),
        }
    });
}
