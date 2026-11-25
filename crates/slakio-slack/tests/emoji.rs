//! A workspace's own emoji against the fake Slack: `emoji.list` is one background request of the
//! budget, read into what each name draws (aliases followed with a cap and a cycle guard, an
//! alias of a standard emoji kept as its name, a picture anywhere but the emoji host left out);
//! an emoji's picture is fetched from the emoji host only, with nothing of the session, each ask
//! and each redirect hop counted, a redirect followed only to another address on the emoji host.

use serde_json::json;
use slakio_core::backend::{BackendHandle, Command, Envelope, Event, FailKind, Generation, Request};
use slakio_core::images::emoji::{ALIAS_DEPTH, Custom};
use slakio_core::images::{AVATAR_HOSTS, BYTES_MAX, EMOJI_HOSTS, Source};
use slakio_core::model::FILE_HOST;
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
    team: String,
    id: u64,
}

impl Run {
    async fn with(limits: Limits) -> Self {
        let world = World::demo();
        let team = world.snapshot().workspaces[0].id.as_str().to_string();
        let fake = FakeSlack::start(world).await.unwrap();
        let governor = Governor::new(limits, Box::new(FakeClock::default()), true);
        let client = Client::new(
            Credentials::new(TOKEN, "cookie-TEST"),
            Endpoints::local(fake.addr()).unwrap(),
            Arc::clone(&governor),
        );
        Self { fake, handle: slakio_slack::serve(client), governor, team, id: 0 }
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

    async fn emoji(&mut self) -> Result<Vec<(String, Custom)>, FailKind> {
        match self.ask(Command::Emoji).await {
            Event::Emoji { emoji } => Ok(emoji),
            Event::Failed { kind, .. } => Err(kind),
            other => panic!("{other:?}"),
        }
    }

    async fn picture(&mut self, source: Source) -> Result<Vec<u8>, FailKind> {
        match self.ask(Command::Image { source, max: BYTES_MAX, list: false }).await {
            Event::Image { bytes } => Ok(bytes),
            Event::Failed { kind, .. } => Err(kind),
            other => panic!("{other:?}"),
        }
    }

    fn requests(&self, method: &str) -> Vec<slakio_fake::Logged> {
        self.fake.log().into_iter().filter(|l| l.method() == method).collect()
    }

    fn blob(&self) -> Source {
        Source::Emoji(slakio_world::emoji::picture(&self.team, "party-blob"))
    }
}

fn header<'a>(l: &'a slakio_fake::Logged, name: &str) -> Option<&'a str> {
    l.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

fn api(code: &str) -> FailKind {
    FailKind::Api { code: code.to_string() }
}

fn redirect(at: &str) -> Fault {
    Fault::Raw { status: 302, headers: vec![("location".to_string(), at.to_string())], body: Vec::new() }
}

fn get(emoji: &[(String, Custom)], name: &str) -> Option<Custom> {
    emoji.iter().find(|(n, _)| n == name).map(|(_, c)| c.clone())
}

#[test]
fn the_list_is_one_background_request_read_into_what_each_name_draws() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let emoji = r.emoji().await.unwrap();
        let blob = slakio_world::emoji::picture(&r.team, "party-blob");
        assert_eq!(get(&emoji, "party-blob"), Some(Custom::Picture(blob.clone())));
        assert_eq!(get(&emoji, "blob-dance"), Some(Custom::Picture(blob)), "an alias of it");
        assert_eq!(get(&emoji, "yes"), Some(Custom::Standard("white_check_mark".into())), "an alias of a standard one");
        for gone in ["loop-a", "loop-b", "far-away", "plain"] {
            assert_eq!(get(&emoji, gone), None, "{gone}: a cycle, another host, http");
        }
        assert_eq!(r.requests("emoji.list").len(), 1);
        assert_eq!(r.governor.sent(), 1, "one request counted");
        assert!(r.requests(PICTURES).is_empty(), "no picture until one is drawn");
    });
}

#[test]
fn the_list_takes_the_background_share_and_the_switch_stops_it() {
    rt().block_on(async {
        let limits = Limits { background_per_minute: 1, lane_background_per_minute: 1, ..Limits::default() };
        let mut r = Run::with(limits).await;
        let blob = r.blob();
        assert!(r.picture(blob).await.is_ok(), "the share's one request");
        assert_eq!(r.emoji().await, Err(FailKind::CapReached), "the list is background too");
        r.governor.stop();
        assert!(matches!(r.emoji().await, Err(FailKind::Offline)));
        assert!(r.requests("emoji.list").is_empty());
    });
}

#[test]
fn aliases_are_followed_to_the_cap_and_no_further_and_cycles_draw_nothing() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let pic = slakio_world::emoji::picture(&r.team, "deep-0");
        let mut list = serde_json::Map::new();
        list.insert("deep-0".into(), json!(pic));
        for i in 1..=ALIAS_DEPTH + 1 {
            list.insert(format!("deep-{i}"), json!(format!("alias:deep-{}", i - 1)));
        }
        list.insert("c1".into(), json!("alias:c2"));
        list.insert("c2".into(), json!("alias:c3"));
        list.insert("c3".into(), json!("alias:c1"));
        list.insert("odd".into(), json!(7));
        list.insert("bad name".into(), json!(pic));
        r.fake.fail_next("emoji.list", Fault::Answer(json!({"ok": true, "emoji": list})));
        let emoji = r.emoji().await.unwrap();
        assert_eq!(get(&emoji, &format!("deep-{ALIAS_DEPTH}")), Some(Custom::Picture(pic)));
        assert_eq!(get(&emoji, &format!("deep-{}", ALIAS_DEPTH + 1)), None, "past the cap");
        let names: Vec<&str> = emoji.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names.len(), ALIAS_DEPTH + 1, "{names:?}: no cycle, no odd value, no bad name");
    });
}

#[test]
fn an_emoji_s_picture_goes_bare_to_the_emoji_host_and_each_ask_counts() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let blob = r.blob();
        let bytes = r.picture(blob.clone()).await.unwrap();
        assert_eq!(&bytes[..4], b"\x89PNG");
        // Two hops on the emoji host are followed, each asked bare and counted.
        let team = r.team.clone();
        let on = |name: &str| slakio_world::emoji::picture(&team, name);
        r.fake.fail_next(PICTURES, redirect(&on("hop-1")));
        r.fake.fail_next(PICTURES, redirect(&on("hop-2")));
        assert!(r.picture(blob.clone()).await.is_ok());
        let log = r.requests(PICTURES);
        assert_eq!(log.len(), 4, "the picture, then it and two hops");
        for l in &log {
            for secret in ["cookie", "authorization", "origin", "referer"] {
                assert_eq!(header(l, secret), None, "{secret} on {}", l.path);
            }
            assert!(l.headers.iter().all(|(_, v)| !v.contains(TOKEN) && !v.contains("cookie-TEST")));
            assert!(l.body.is_empty(), "a GET");
        }
        assert_eq!(r.governor.sent(), 4, "each ask and each hop counts");
        // A third hop is refused, though on the emoji host.
        for name in ["h1", "h2", "h3"] {
            r.fake.fail_next(PICTURES, redirect(&on(name)));
        }
        assert_eq!(r.picture(blob).await, Err(api("redirected_elsewhere")));
        assert_eq!(r.requests(PICTURES).len(), 7, "the third hop never asked");
    });
}

#[test]
fn an_emoji_never_comes_from_elsewhere_nor_follows_a_redirect_there() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let blob = r.blob();
        let host = EMOJI_HOSTS[0];
        let path = blob.address().split_once(".com").unwrap().1.to_string();
        for bad in [
            "https://evil.example/e.png".to_string(),
            format!("http://{host}{path}"),
            format!("https://{host}.evil.com{path}"),
            format!("https://evil@{host}{path}"),
            format!("https://{host}@evil.example{path}"),
            format!("https://{host}:8443{path}"),
            format!("https://{}{path}", AVATAR_HOSTS[0]),
            format!("https://{FILE_HOST}{path}"),
        ] {
            assert_eq!(r.picture(Source::Emoji(bad.clone())).await, Err(api("not_a_picture_address")), "{bad}");
            r.fake.fail_next(PICTURES, redirect(&bad));
            assert_eq!(r.picture(blob.clone()).await, Err(api("redirected_elsewhere")), "{bad}");
        }
        assert_eq!(r.requests(PICTURES).len(), 8, "only the emoji itself each time: nothing asked elsewhere");
        assert_eq!(r.governor.sent(), 8);
        // An emoji's address passed off as a photo is refused: its kind is checked again.
        assert_eq!(r.picture(Source::Avatar(blob.address().to_string())).await, Err(api("not_a_picture_address")));
        assert_eq!(r.governor.sent(), 8);
    });
}

#[test]
fn an_answer_that_is_not_slack_s_json_is_told_apart_from_the_network() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.fake.fail_next("emoji.list", Fault::Malformed);
        assert_eq!(r.emoji().await, Err(api("not_slack_json")));
        r.fake.fail_next("emoji.list", Fault::Status { status: 503, retry_after: None });
        assert_eq!(r.emoji().await, Err(FailKind::Network), "a server's trouble stays the network");
        r.fake.fail_next("emoji.list", Fault::Api("internal_error".to_string()));
        assert_eq!(r.emoji().await, Err(api("internal_error")));
        assert!(r.emoji().await.is_ok());
    });
}
