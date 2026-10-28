//! Live updates against the fake Slack: the socket opens once the workspace booted, as the web
//! client opens it; messages posted, edited and deleted and conversations read elsewhere arrive
//! as they happen; the client sends nothing over it but pongs and a close; a socket dropped,
//! closed or silent opens again after a pause within its bounds and asks to catch up; with the
//! network off none opens, and switching it off ends one; every opening counts against the cap;
//! one workspace's failing socket leaves another's alone. Events are synthetic, written here.

use serde_json::json;
use slakio_core::backend::{BackendHandle, Command, Connection, Envelope, Event, FailKind, Generation, Live, Request};
use slakio_core::model::{Target, Ts};
use slakio_core::secret::Credentials;
use slakio_fake::FakeSlack;
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_slack::{Client, Realtime};
use slakio_world::World;
use std::sync::Arc;
use std::time::{Duration, Instant};

const TOKEN: &str = concat!("xo", "xc-TEST");

/// Quick pauses, so the tests do not wait seconds.
const QUICK: Realtime = Realtime {
    first_retry: Duration::from_millis(40),
    longest_retry: Duration::from_millis(160),
    steady: Duration::from_secs(10),
    silence: Duration::from_secs(10),
};

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

struct Run {
    fake: FakeSlack,
    handle: BackendHandle,
    governor: Arc<Governor>,
    world: World,
    team: usize,
    id: u64,
    /// Pushed events that came while an answer was awaited.
    pushed: Vec<Live>,
}

impl Run {
    async fn new(realtime: Realtime) -> Self {
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        Self::team(0, realtime, governor).await
    }

    async fn team(team: usize, realtime: Realtime, governor: Arc<Governor>) -> Self {
        let fake = FakeSlack::start_team(World::demo(), team).await.unwrap();
        let endpoints = Endpoints::local(fake.addr()).unwrap();
        let client = Client::new(Credentials::new(TOKEN, "cookie-TEST"), endpoints, Arc::clone(&governor));
        let handle = slakio_slack::serve_live(client, realtime);
        Self { fake, handle, governor, world: World::demo(), team, id: 0, pushed: Vec::new() }
    }

    async fn ask(&mut self, command: Command) -> Event {
        self.id += 1;
        self.handle.send(Request { id: Generation(self.id), workspace: None, command }).unwrap();
        loop {
            let e = self.envelope().await.expect("an answer");
            match e.event {
                Event::Live(live) => self.pushed.push(*live),
                event => {
                    assert_eq!(e.generation, Generation(self.id));
                    return event;
                }
            }
        }
    }

    async fn envelope(&mut self) -> Option<Envelope> {
        tokio::time::timeout(Duration::from_secs(10), self.handle.rx.recv()).await.ok().flatten()
    }

    /// The next pushed event (those pushed while an answer was awaited first).
    async fn live(&mut self) -> Live {
        if !self.pushed.is_empty() {
            return self.pushed.remove(0);
        }
        let e = self.envelope().await.expect("a live update");
        assert_eq!(e.generation, Generation::PUSHED);
        match e.event {
            Event::Live(live) => *live,
            other => panic!("{other:?}"),
        }
    }

    /// Nothing pushed for `wait`.
    async fn quiet(&mut self, wait: Duration) {
        assert!(self.pushed.is_empty(), "{:?}", self.pushed);
        if let Ok(Some(e)) = tokio::time::timeout(wait, self.handle.rx.recv()).await {
            panic!("pushed: {e:?}");
        }
    }

    async fn boot(&mut self) {
        assert!(matches!(self.ask(Command::Boot).await, Event::Booted(_)));
    }

    async fn connected(&mut self) -> bool {
        match self.live().await {
            Live::Connection { state: Connection::Connected { catch_up }, .. } => catch_up,
            other => panic!("{other:?}"),
        }
    }

    async fn lost(&mut self) {
        assert!(matches!(self.live().await, Live::Connection { state: Connection::Lost, .. }));
    }

    fn ws(&self) -> &slakio_core::model::Workspace {
        &self.world.snapshot().workspaces[self.team]
    }

    /// A conversation of the workspace, by name.
    fn channel(&self, name: &str) -> String {
        let s = self.world.snapshot();
        s.conversations.iter().find(|c| c.workspace == self.ws().id && c.name == name).unwrap().id.to_string()
    }

    /// Someone of the workspace who is not the user.
    fn other(&self) -> String {
        let s = self.world.snapshot();
        s.users.iter().find(|u| u.workspace == self.ws().id && u.id != self.ws().me && !u.bot).unwrap().id.to_string()
    }

    fn calls(&self, method: &str) -> usize {
        self.fake.methods().iter().filter(|m| *m == method).count()
    }
}

async fn until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[test]
fn the_socket_opens_after_the_boot_as_the_web_client_opens_it() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(r.fake.upgrades().is_empty(), "nothing before the workspace booted");
        r.boot().await;
        assert!(!r.connected().await, "the first socket has nothing to catch up on");
        let up = r.fake.upgrades();
        assert_eq!(up.len(), 1);
        let names: Vec<&str> = up[0].headers.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            names.iter().map(|n| n.to_ascii_lowercase()).collect::<Vec<_>>(),
            [
                "host",
                "connection",
                "pragma",
                "cache-control",
                "user-agent",
                "upgrade",
                "origin",
                "sec-websocket-version",
                "accept-language",
                "cookie",
                "sec-websocket-key"
            ]
        );
        assert_eq!(up[0].path, "/");
        let team = r.ws().id.to_string();
        assert!(up[0].query.starts_with(&format!("token={TOKEN}&sync_desync=1&")), "{}", up[0].query);
        assert!(up[0].query.contains(&format!("&gateway_server={team}-1&")), "{}", up[0].query);
        // It is not an API request.
        assert!(r.fake.methods().iter().all(|m| m != "/"));
    });
}

#[test]
fn posts_replies_edits_deletions_and_reads_elsewhere_arrive_as_they_happen() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.boot().await;
        r.connected().await;
        let (c, who, me) = (r.channel("general"), r.other(), r.ws().me.to_string());
        r.fake.push(&json!({"type": "user_typing", "channel": c, "user": who}));
        r.fake.push(&json!({"type": "message", "channel": c, "user": who, "text": format!("hi <@{me}>"),
            "ts": "1900000000.000100", "team": r.ws().id.to_string()}));
        let Live::Message { conversation, thread, changed, mention, message, .. } = r.live().await else { panic!() };
        assert_eq!((conversation.as_str(), thread, changed, mention), (c.as_str(), None, false, true));
        assert_eq!(message.ts, Ts(1_900_000_000_000_100));
        r.fake.push(&json!({"type": "message", "channel": c, "user": who, "text": "a reply",
            "ts": "1900000001.000000", "thread_ts": "1900000000.000100"}));
        let Live::Message { thread, mention, .. } = r.live().await else { panic!() };
        assert_eq!((thread, mention), (Some(Ts(1_900_000_000_000_100)), false));
        r.fake.push(&json!({"type": "message", "subtype": "message_changed", "channel": c, "hidden": true,
            "message": {"user": who, "text": "hi again", "ts": "1900000000.000100", "edited": {"ts": "1900000002.000000"}}}));
        let Live::Message { changed, message, .. } = r.live().await else { panic!() };
        assert!(changed && message.edited);
        r.fake.push(&json!({"type": "message", "subtype": "message_deleted", "channel": c,
            "deleted_ts": "1900000001.000000", "previous_message": {"ts": "1900000001.000000", "thread_ts": "1900000000.000100"}}));
        assert_eq!(
            r.live().await,
            Live::Deleted {
                workspace: r.ws().id.clone(),
                conversation: conversation.clone(),
                thread: Some(Ts(1_900_000_000_000_100)),
                ts: Ts(1_900_000_001_000_000)
            }
        );
        r.fake.push(&json!({"type": "channel_marked", "channel": c, "ts": "1900000000.000100",
            "unread_count_display": 0, "mention_count": 0}));
        assert_eq!(
            r.live().await,
            Live::Marked { workspace: r.ws().id.clone(), conversation, unread: 0, mentions: 0 }
        );
    });
}

#[test]
fn a_new_author_is_named_once_a_socket() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.boot().await;
        r.connected().await;
        let c = r.channel("general");
        // Someone the boot did not name (not a DM peer).
        let s = r.world.snapshot();
        let peers: Vec<String> = s
            .conversations
            .iter()
            .filter_map(|c| match &c.kind {
                slakio_core::model::ConversationKind::Dm { user } => Some(user.to_string()),
                _ => None,
            })
            .collect();
        let me = r.ws().me.clone();
        let who = s
            .users
            .iter()
            .find(|u| u.workspace == r.ws().id && u.id != me && !u.bot && !peers.contains(&u.id.to_string()))
            .unwrap()
            .id
            .to_string();
        let before = r.calls("users/info");
        for (i, ts) in ["1900000000.000100", "1900000000.000200"].into_iter().enumerate() {
            r.fake.push(&json!({"type": "message", "channel": c, "user": who, "text": "x", "ts": ts}));
            let Live::Message { users, .. } = r.live().await else { panic!() };
            if i == 0 {
                assert!(users.iter().any(|u| u.id.as_str() == who), "named: {users:?}");
            }
        }
        assert_eq!(r.calls("users/info"), before + 1, "looked up once");
    });
}

#[test]
fn nothing_goes_out_over_the_socket_but_pongs_and_a_close() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.boot().await;
        r.connected().await;
        let c = r.channel("general");
        for i in 0..20 {
            let ts = format!("19000000{i:02}.000100");
            r.fake.push(&json!({"type": "message", "channel": c, "user": r.other(), "text": "x", "ts": ts}));
            r.live().await;
        }
        r.fake.ping(b"p1");
        r.fake.ping(b"");
        until("two pongs", || r.fake.client_frames().len() >= 2).await;
        let frames = r.fake.client_frames();
        assert!(frames.iter().all(|f| f.masked), "{frames:?}");
        let got: Vec<(u8, Vec<u8>)> = frames.iter().map(|f| (f.opcode, f.payload.clone())).collect();
        assert_eq!(got, [(0xA, b"p1".to_vec()), (0xA, Vec::new())]);
        // Slack closes: it is answered with a close, and nothing else is written.
        r.fake.close_sockets();
        r.lost().await;
        until("the close", || r.fake.client_frames().len() >= 3).await;
        let frames = r.fake.client_frames();
        assert_eq!((frames[2].opcode, frames[2].payload.clone()), (0x8, 1001u16.to_be_bytes().to_vec()));
        assert!(r.connected().await, "opened again, to catch up");
        assert_eq!(r.fake.client_frames().len(), 3);
    });
}

#[test]
fn a_dropped_socket_opens_again_and_catching_up_reads_one_page_in_the_background() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.boot().await;
        r.connected().await;
        let calls = r.fake.methods().len();
        r.fake.drop_sockets();
        r.lost().await;
        assert!(r.connected().await, "what was missed should be read again");
        assert_eq!(r.fake.upgrades().len(), 2);
        assert_eq!(r.fake.methods().len(), calls, "opening again asks the API nothing by itself");
        let c = r.channel("general");
        let target = Target::Conversation {
            workspace: r.ws().id.clone(),
            conversation: slakio_core::model::ConversationId::new(&c),
        };
        let Event::History(page) = r.ask(Command::Catchup { target: target.clone(), limit: 200 }).await else {
            panic!()
        };
        assert_eq!(page.target, target);
        assert!(!page.messages.is_empty() && page.messages.len() <= 28);
        let log = r.fake.log();
        let history: Vec<_> = log.iter().filter(|l| l.method() == "conversations.history").collect();
        assert_eq!(history.len(), 1);
        let fields = history[0].fields();
        assert!(fields.contains(&("limit", "28")) && !fields.iter().any(|(k, _)| *k == "latest"), "{fields:?}");
        assert_eq!(r.calls("conversations.view"), 0, "never an open, which could cancel one");
        // A thread is not read again this way.
        let thread = Target::Thread {
            workspace: r.ws().id.clone(),
            conversation: slakio_core::model::ConversationId::new(&c),
            thread: Ts(1),
        };
        assert!(matches!(r.ask(Command::Catchup { target: thread, limit: 28 }).await, Event::Failed { .. }));
    });
}

#[test]
fn refused_openings_are_tried_again_after_pauses_within_their_bounds() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.fake.refuse_sockets(4);
        let start = Instant::now();
        r.boot().await;
        // Said once, however many attempts fail.
        r.lost().await;
        assert!(!r.connected().await, "never open before: nothing to catch up on");
        let took = start.elapsed();
        assert_eq!(r.fake.upgrades().len(), 5);
        // Pauses of 20–40, 40–80, 80–160 and 80–160 ms.
        assert!(took >= Duration::from_millis(220), "{took:?}");
        assert!(took < Duration::from_secs(5), "{took:?}");
    });
}

#[test]
fn a_silent_socket_is_lost_closed_and_opened_again() {
    rt().block_on(async {
        let mut r = Run::new(Realtime { silence: Duration::from_millis(300), ..QUICK }).await;
        r.boot().await;
        r.connected().await;
        r.lost().await;
        until("the close", || !r.fake.client_frames().is_empty()).await;
        let frames = r.fake.client_frames();
        assert_eq!(frames.iter().map(|f| f.opcode).collect::<Vec<_>>(), [0x8], "closed from this end");
        assert!(r.connected().await);
        // A ping keeps one open.
        r.fake.ping(b"");
        tokio::time::sleep(Duration::from_millis(200)).await;
        r.fake.ping(b"");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(r.fake.upgrades().len(), 2);
    });
}

#[test]
fn with_the_network_off_no_socket_ever_opens() {
    rt().block_on(async {
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), false);
        let mut r = Run::team(0, QUICK, governor).await;
        let Event::Failed { kind, .. } = r.ask(Command::Boot).await else { panic!() };
        assert_eq!(kind, FailKind::Offline);
        r.quiet(Duration::from_millis(400)).await;
        assert!(r.fake.upgrades().is_empty() && r.fake.log().is_empty());
    });
}

#[test]
fn switching_the_network_off_ends_the_socket_for_good() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.boot().await;
        r.connected().await;
        assert_eq!(r.fake.open_sockets(), 1);
        r.governor.stop();
        until("the socket closed", || r.fake.open_sockets() == 0).await;
        r.quiet(Duration::from_millis(400)).await;
        assert_eq!(r.fake.upgrades().len(), 1, "never opened again");
    });
}

#[test]
fn every_opening_counts_against_the_cap_which_stops_them() {
    rt().block_on(async {
        let limits = Limits { per_process: 30, ..Limits::default() };
        let governor = Governor::new(limits, Box::new(FakeClock::default()), true);
        let quick =
            Realtime { first_retry: Duration::from_millis(2), longest_retry: Duration::from_millis(4), ..QUICK };
        let mut r = Run::team(0, quick, Arc::clone(&governor)).await;
        r.fake.refuse_sockets(usize::MAX);
        r.boot().await;
        until("the cap reached", || !governor.is_on()).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let (api, sockets) = (r.fake.log().len(), r.fake.upgrades().len());
        assert!(sockets >= 2, "{sockets}");
        assert_eq!(api + sockets, 30, "every opening counted: {api} + {sockets}");
        assert_eq!(governor.sent(), 30);
    });
}

#[test]
fn one_workspace_s_failing_socket_leaves_the_other_alone() {
    rt().block_on(async {
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let mut a = Run::team(0, QUICK, Arc::clone(&governor)).await;
        let mut b = Run::team(1, QUICK, Arc::clone(&governor)).await;
        a.boot().await;
        b.boot().await;
        a.connected().await;
        b.connected().await;
        a.fake.refuse_sockets(usize::MAX);
        a.fake.drop_sockets();
        a.lost().await;
        for i in 0..5 {
            let ts = format!("19000000{i:02}.000100");
            let c = b.channel("general");
            b.fake.push(&json!({"type": "message", "channel": c, "user": b.other(), "text": "x", "ts": ts}));
            let Live::Message { workspace, .. } = b.live().await else { panic!() };
            assert_eq!(workspace, b.ws().id);
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(a.fake.upgrades().len() > 2, "A keeps trying");
        assert_eq!(b.fake.upgrades().len(), 1, "B's socket stayed open");
        a.quiet(Duration::from_millis(200)).await;
    });
}
