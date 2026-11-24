//! Pictures against the fake Slack: a file's smaller copy fetched from the file host with the
//! session's cookie and never the token; a profile photo fetched from an avatar host with
//! neither; only what says it is an image read, at most the bytes asked; a copy's redirects
//! followed only on the file host, a photo's (at most two) only to an avatar host, each hop
//! checked again and counted; addresses elsewhere never asked; every ask a background request of the
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
fn redirects_stay_on_the_file_host_and_a_photo_follows_none_elsewhere() {
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
        r.fake.fail_next(PICTURES, to("https://evil.example/p.png"));
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

fn redirect(at: &str) -> Fault {
    Fault::Raw { status: 302, headers: vec![("location".to_string(), at.to_string())], body: Vec::new() }
}

/// `photo`'s path on avatar host `host`.
fn on(host: &str, photo: &Source) -> String {
    format!("https://{host}{}", photo.address().split_once(".com").unwrap().1)
}

#[test]
fn a_photo_follows_at_most_two_redirects_to_avatar_hosts_each_counted_and_bare() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let (_, photo) = r.sources();
        // Gravatar to Slack's default avatar, then once more on Slack's avatar host.
        r.fake.fail_next(PICTURES, redirect(&on(AVATAR_HOSTS[2], &photo)));
        r.fake.fail_next(PICTURES, redirect(&on(AVATAR_HOSTS[0], &photo)));
        let bytes = r.picture(photo.clone(), BYTES_MAX).await.unwrap();
        assert_eq!(&bytes[..4], b"\x89PNG");
        let log = r.picture_requests();
        assert_eq!(log.len(), 3, "the photo and two hops");
        for l in &log {
            for secret in ["cookie", "authorization", "origin", "referer"] {
                assert_eq!(header(l, secret), None, "{secret} on {}", l.path);
            }
            assert!(l.headers.iter().all(|(_, v)| !v.contains(TOKEN) && !v.contains("cookie-TEST")));
            assert!(l.body.is_empty(), "a GET");
        }
        assert_eq!(r.governor.sent(), 3, "each hop counts");
        // A third hop is refused, though to an avatar host.
        for host in [AVATAR_HOSTS[2], AVATAR_HOSTS[0], AVATAR_HOSTS[2]] {
            r.fake.fail_next(PICTURES, redirect(&on(host, &photo)));
        }
        assert_eq!(r.picture(photo, BYTES_MAX).await, Err(api("redirected_elsewhere")));
        assert_eq!(r.picture_requests().len(), 6, "the third hop never asked");
        assert_eq!(r.governor.sent(), 6);
    });
}

#[test]
fn a_photo_never_follows_a_redirect_to_an_address_a_photo_may_not_come_from() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let (thumb, photo) = r.sources();
        let slack = AVATAR_HOSTS[2];
        let path = photo.address().split_once(".com").unwrap().1.to_string();
        // The fake serves the photo's path on any host.
        let gravatar = format!("https://{}{path}?s=72", AVATAR_HOSTS[1]);
        for to in [
            "https://evil.example/a.png".to_string(),
            format!("http://{slack}{path}"),
            format!("https://{slack}.evil.com{path}"),
            format!("https://evil@{slack}{path}"),
            format!("https://{slack}@evil.example{path}"),
            format!("https://{slack}:8443{path}"),
            format!("https://{slack}:443{path}"),
            format!("{gravatar}&d=https%3A%2F%2Fevil.example%2Fa.png"),
            format!("{gravatar}&d=https://evil.example/a.png"),
            thumb.address().to_string(),
            path.clone(),
            format!("//{slack}{path}"),
            String::new(),
        ] {
            r.fake.fail_next(PICTURES, redirect(&to));
            assert_eq!(r.picture(photo.clone(), BYTES_MAX).await, Err(api("redirected_elsewhere")), "{to:?}");
        }
        // A redirect with no address at all.
        r.fake.fail_next(PICTURES, Fault::Raw { status: 302, headers: Vec::new(), body: Vec::new() });
        assert_eq!(r.picture(photo.clone(), BYTES_MAX).await, Err(api("redirected_elsewhere")));
        assert_eq!(r.picture_requests().len(), 14, "only the photo itself each time: nothing asked elsewhere");
        assert_eq!(r.governor.sent(), 14);
        // Gravatar sending to Slack's default avatar is followed.
        r.fake.fail_next(PICTURES, redirect(&format!("{gravatar}&d=https%3A%2F%2F{slack}%2Fimg%2Fava_0010-72.png")));
        assert!(r.picture(photo, BYTES_MAX).await.is_ok());
        assert_eq!(r.picture_requests().len(), 16);
    });
}

#[test]
fn the_user_s_own_photo_is_looked_up_when_the_start_bundle_has_none() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let s = match r.ask(Command::Boot).await {
            Event::Booted(s) => *s,
            other => panic!("{other:?}"),
        };
        let me = &s.workspaces[0].me;
        let world = r.world.snapshot();
        let want = world.users.iter().find(|u| &u.id == me).and_then(|u| u.avatar.clone());
        assert!(want.is_some(), "the world gives the user a photo");
        let got: Vec<_> = s.users.iter().filter(|u| &u.id == me).collect();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].avatar, want, "looked up, not dropped");
        let asked =
            r.fake.log().into_iter().filter(|l| l.method() == "users/info").any(|l| l.body.contains(me.as_str()));
        assert!(asked, "asked of the edge cache with the DMs' people");
    });
}

/// A Gravatar address of `photo`'s path whose fallback (`d=`) is `fallback`, as written.
fn gravatar_of(photo: &Source, fallback: &str) -> Source {
    let path = photo.address().split_once(".com").unwrap().1;
    Source::Avatar(format!("https://{}{path}?s=72&d={fallback}", AVATAR_HOSTS[1]))
}

#[test]
fn gravatar_sending_elsewhere_asks_the_slack_fallback_it_named_and_nothing_elsewhere() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let (_, photo) = r.sources();
        let slack = on(AVATAR_HOSTS[2], &photo);
        let encoded = slack.replace(':', "%3A").replace('/', "%2F");
        let cdn = format!("https://i0.wp.com/{}?ssl=1", &slack["https://".len()..]);
        r.fake.fail_next(PICTURES, redirect(&cdn));
        let bytes = r.picture(gravatar_of(&photo, &encoded), BYTES_MAX).await.unwrap();
        assert_eq!(&bytes[..4], b"\x89PNG");
        let log = r.picture_requests();
        assert_eq!(log.len(), 2, "Gravatar, then the Slack default it named: {log:?}");
        assert_eq!(r.governor.sent(), 2, "each hop counts");
        assert_eq!(log[1].path, slack.split_once(".com").unwrap().1, "the fallback's own path");
        for l in &log {
            assert!(l.headers.iter().all(|(_, v)| !v.contains("wp.com")), "nothing of the CDN");
            assert_eq!(header(l, "cookie"), None);
        }
        // A fallback that redirects elsewhere in its turn fails: no third ask.
        r.fake.fail_next(PICTURES, redirect(&cdn));
        r.fake.fail_next(PICTURES, redirect(&cdn));
        assert_eq!(r.picture(gravatar_of(&photo, &encoded), BYTES_MAX).await, Err(api("redirected_elsewhere")));
        assert_eq!(r.picture_requests().len(), 4);
        // A keyword fallback (or none) has no Slack address to ask: the photo fails, one ask.
        for keyword in ["mp", "404"] {
            r.fake.fail_next(PICTURES, redirect(&cdn));
            let source = gravatar_of(&photo, keyword);
            assert_eq!(r.picture(source, BYTES_MAX).await, Err(api("redirected_elsewhere")), "{keyword}");
        }
        assert_eq!(r.picture_requests().len(), 6, "only Gravatar each time");
        assert_eq!(r.governor.sent(), 6);
    });
}

#[test]
fn the_gravatar_fallback_stays_within_the_hop_cap() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let (_, photo) = r.sources();
        let slack = on(AVATAR_HOSTS[2], &photo);
        let encoded = slack.replace(':', "%3A").replace('/', "%2F");
        let gravatar = gravatar_of(&photo, &encoded);
        // Gravatar to Gravatar (allowed), to Gravatar again, then elsewhere: two hops used, no
        // third for the fallback.
        r.fake.fail_next(PICTURES, redirect(gravatar.address()));
        r.fake.fail_next(PICTURES, redirect(gravatar.address()));
        r.fake.fail_next(PICTURES, redirect("https://i0.wp.com/x.png"));
        assert_eq!(r.picture(gravatar, BYTES_MAX).await, Err(api("redirected_elsewhere")));
        assert_eq!(r.picture_requests().len(), 3);
        assert_eq!(r.governor.sent(), 3);
    });
}

#[test]
fn the_user_s_own_photo_a_page_tells_reaches_the_screen_when_the_start_had_none() {
    rt().block_on(async {
        let mut r = Run::new().await;
        // The start's lookup fails: the user's own record has no photo.
        r.fake.fail_next("users/info", Fault::Status { status: 503, retry_after: None });
        let Event::Booted(s) = r.ask(Command::Boot).await else { panic!("booted") };
        let me = s.workspaces[0].me.clone();
        assert_eq!(s.users.iter().find(|u| u.id == me).and_then(|u| u.avatar.clone()), None, "no photo at the start");
        let photo = format!("https://{}/avatars/me_72.png", AVATAR_HOSTS[0]);
        let view = json!({
            "ok": true,
            "history": {"messages": [{"type": "message", "user": me.as_str(), "text": "mine", "ts": "1767603600.000100"}], "has_more": false},
            "users": [{"id": me.as_str(), "team_id": s.workspaces[0].id.as_str(), "name": "me", "profile": {"display_name": "Me", "image_72": photo}}],
            "channels": [],
        });
        r.fake.fail_next("conversations.view", Fault::Answer(view));
        let lookups = r.fake.log().iter().filter(|l| l.method() == "users/info").count();
        let Event::History(page) = r.ask(Command::History { target: r.target("general"), before: None, limit: 20 }).await
        else {
            panic!("a page")
        };
        let mine: Vec<_> = page.users.iter().filter(|u| u.id == me).collect();
        assert_eq!(mine.len(), 1, "the user's own record comes along");
        assert_eq!(mine[0].avatar.as_deref(), Some(photo.as_str()));
        let after = r.fake.log().iter().filter(|l| l.method() == "users/info").count();
        assert_eq!(after, lookups, "nothing asked for it");
    });
}

#[test]
fn a_dm_peer_s_photo_comes_with_the_boot_before_the_dm_is_opened() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let s = match r.ask(Command::Boot).await {
            Event::Booted(s) => *s,
            other => panic!("{other:?}"),
        };
        let world = r.world.snapshot();
        let peers: Vec<_> = s
            .conversations
            .iter()
            .filter_map(|c| match &c.kind {
                slakio_core::model::ConversationKind::Dm { user } => Some(user.clone()),
                _ => None,
            })
            .collect();
        let mut pictured = 0;
        for peer in &peers {
            let want = world.users.iter().find(|u| &u.id == peer).and_then(|u| u.avatar.clone());
            let want = want.filter(|_| !peer.as_str().ends_with("666"));
            let got = s.users.iter().find(|u| &u.id == peer).and_then(|u| u.avatar.clone());
            pictured += usize::from(want.is_some());
            assert_eq!(got, want, "{}: the photo comes with the boot", peer.as_str());
        }
        assert!(pictured > 0, "the world gives some DM peers a photo");
        let methods = r.fake.methods();
        assert!(!methods.iter().any(|m| m.starts_with("conversations.")), "no DM opened: {methods:?}");
        let lookups: Vec<serde_json::Value> = r
            .fake
            .log()
            .into_iter()
            .filter(|l| l.method() == "users/info")
            .map(|l| serde_json::from_str(&l.body).unwrap())
            .collect();
        assert!(!lookups.is_empty());
        for body in &lookups {
            assert_eq!(body.get("include_profile_only_if_stale"), None, "the whole profile is asked for");
            assert!(body["updated_ids"].as_object().unwrap().values().all(|v| v == &json!(0)), "{body}");
        }
    });
}
