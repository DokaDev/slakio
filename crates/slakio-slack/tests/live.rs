//! Live updates against the fake Slack: the socket opens once the workspace booted, as the web
//! client opens it; messages posted, edited and deleted and conversations read elsewhere arrive
//! as they happen; the client sends nothing over it but pongs and a close; a socket dropped,
//! closed or silent opens again after a pause within its bounds and asks to catch up; with the
//! network off none opens, and switching it off ends one; every opening counts against the cap;
//! one workspace's failing socket leaves another's alone; an opening the budget has no room for
//! is said as held; a machine that slept loses its socket at once; after a loss the unread counts
//! are read again, once; the network switched off closes the socket, and on opens it again.
//! Events are synthetic, written here.

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
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

const TOKEN: &str = concat!("xo", "xc-TEST");

/// Quick pauses, so the tests do not wait seconds.
const QUICK: Realtime = Realtime {
    first_retry: Duration::from_millis(40),
    longest_retry: Duration::from_millis(160),
    steady: Duration::from_secs(10),
    silence: Duration::from_secs(10),
    wake_check: Duration::from_secs(5),
    wake_gap: Duration::from_secs(10),
    wall: SystemTime::now,
};

/// How far the tests' wall clock is ahead of the real one: moved on, it is as if the machine
/// slept that long (the monotonic clock did not move).
static SLEPT: AtomicU64 = AtomicU64::new(0);

fn wall_after_sleep() -> SystemTime {
    SystemTime::now() + Duration::from_secs(SLEPT.load(Ordering::SeqCst))
}

/// The same for the test of wakes that come one after the other (a test of its own, so the two
/// can run at once).
static SLEPT_AGAIN: AtomicU64 = AtomicU64::new(0);

fn wall_after_sleeps() -> SystemTime {
    SystemTime::now() + Duration::from_secs(SLEPT_AGAIN.load(Ordering::SeqCst))
}

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
    /// The unread counts pushed, kept apart from the other updates.
    counts: Vec<Live>,
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
        Self { fake, handle, governor, world: World::demo(), team, id: 0, pushed: Vec::new(), counts: Vec::new() }
    }

    async fn ask(&mut self, command: Command) -> Event {
        self.id += 1;
        self.handle.send(Request { id: Generation(self.id), workspace: None, command }).unwrap();
        loop {
            let e = self.envelope().await.expect("an answer");
            match e.event {
                Event::Live(live) if matches!(*live, Live::Counts { .. }) => self.counts.push(*live),
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

    /// The next pushed event (those pushed while an answer was awaited first), the unread counts
    /// put aside ([`Run::counted`]).
    async fn live(&mut self) -> Live {
        if !self.pushed.is_empty() {
            return self.pushed.remove(0);
        }
        loop {
            let e = self.envelope().await.expect("a live update");
            assert_eq!(e.generation, Generation::PUSHED);
            match e.event {
                Event::Live(live) if matches!(*live, Live::Counts { .. }) => self.counts.push(*live),
                Event::Live(live) => return *live,
                other => panic!("{other:?}"),
            }
        }
    }

    /// The unread counts pushed next (or put aside already).
    async fn counted(&mut self) -> Live {
        while self.counts.is_empty() {
            let e = self.envelope().await.expect("the counts");
            match e.event {
                Event::Live(live) if matches!(*live, Live::Counts { .. }) => self.counts.push(*live),
                Event::Live(live) => self.pushed.push(*live),
                other => panic!("{other:?}"),
            }
        }
        self.counts.remove(0)
    }

    /// Nothing pushed for `wait` (the unread counts put aside).
    async fn quiet(&mut self, wait: Duration) {
        assert!(self.pushed.is_empty(), "{:?}", self.pushed);
        let deadline = tokio::time::Instant::now() + wait;
        while let Ok(Some(e)) = tokio::time::timeout_at(deadline, self.handle.rx.recv()).await {
            match e.event {
                Event::Live(live) if matches!(*live, Live::Counts { .. }) => self.counts.push(*live),
                other => panic!("pushed: {other:?}"),
            }
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

    /// Read conversation `c` (its newest page), as the UI does when a pane opens it.
    async fn open(&mut self, c: &str) {
        let target = Target::Conversation {
            workspace: self.ws().id.clone(),
            conversation: slakio_core::model::ConversationId::new(c),
        };
        let command = Command::History { target, before: None, limit: 28 };
        assert!(matches!(self.ask(command).await, Event::History(_)));
    }

    /// Push a message by `who` at `ts` into `c`.
    fn post(&self, c: &str, who: &str, ts: &str) {
        self.fake.push(&json!({"type": "message", "channel": c, "user": who, "text": "x", "ts": ts}));
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
fn a_new_author_is_named_once_a_run() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.boot().await;
        r.connected().await;
        let c = r.channel("general");
        r.open(&c).await;
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
        // Nor an author of the page read (its people come with it).
        let (read, _) = r.world.history(&slakio_core::model::ConversationId::new(&c), None, 28);
        let who = s
            .users
            .iter()
            .find(|u| {
                u.workspace == r.ws().id
                    && u.id != me
                    && !u.bot
                    && !peers.contains(&u.id.to_string())
                    && !read.iter().any(|m| m.user == u.id)
            })
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
        // Nor again after the socket opened again.
        r.fake.drop_sockets();
        r.lost().await;
        r.connected().await;
        r.post(&c, &who, "1900000000.000300");
        r.live().await;
        assert_eq!(r.calls("users/info"), before + 1, "looked up once a run");
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
        assert!(matches!(r.counted().await, Live::Counts { .. }));
        assert_eq!(r.fake.methods()[calls..], ["client.counts"], "opening again asks for the counts only");
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
    });
}

#[test]
fn a_thread_is_caught_up_by_reading_what_is_new_in_it() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.boot().await;
        r.connected().await;
        let c = r.channel("general");
        let id = slakio_core::model::ConversationId::new(&c);
        let (page, _) = r.world.history(&id, None, 200);
        let parent = page.iter().find(|m| m.thread.is_some()).expect("a thread in general").ts;
        let thread = Target::Thread { workspace: r.ws().id.clone(), conversation: id, thread: parent };
        let before = r.calls("conversations.replies");
        let Event::History(page) = r.ask(Command::Catchup { target: thread.clone(), limit: 200 }).await else {
            panic!()
        };
        assert_eq!(page.target, thread);
        assert!(page.messages.len() <= 28 && page.messages.iter().all(|m| m.ts >= parent), "{page:?}");
        assert_eq!(r.calls("conversations.replies"), before + 1, "one request");
        assert_eq!(r.calls("conversations.view") + r.calls("conversations.history"), 0, "the thread only");
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
        assert!(r.connected().await, "what came while it was not connected is read again");
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
fn the_kill_switch_ends_the_socket_for_good() {
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

#[test]
fn new_authors_are_named_only_where_the_user_reads_and_in_batches() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.boot().await;
        r.connected().await;
        let (c, elsewhere) = (r.channel("general"), r.channel("random"));
        r.open(&c).await;
        let before = r.calls("users/info");
        // A large workspace: many people nobody named yet, in a conversation the user did not read.
        for i in 0..300 {
            r.post(&elsewhere, &format!("UNEW{i:04}"), &format!("1900000000.{i:06}"));
        }
        for _ in 0..300 {
            let Live::Message { users, .. } = r.live().await else { panic!() };
            assert!(users.is_empty());
        }
        assert_eq!(r.calls("users/info"), before, "never looked up where nobody reads");
        // Where the user reads: looked up together, not one request each, and in order.
        for i in 0..400 {
            r.post(&c, &format!("UOPEN{i:04}"), &format!("1900000001.{i:06}"));
        }
        for i in 0..400 {
            let Live::Message { message, .. } = r.live().await else { panic!() };
            assert_eq!(message.ts, Ts(1_900_000_001_000_000 + i), "in the order they came");
        }
        let lookups = r.calls("users/info") - before;
        assert!((1..=40).contains(&lookups), "{lookups} requests for 400 new authors");
        // Ids Slack does not name are not asked about again.
        for i in 0..400 {
            r.post(&c, &format!("UOPEN{i:04}"), &format!("1900000002.{i:06}"));
            r.live().await;
        }
        assert_eq!(r.calls("users/info") - before, lookups);
        assert!(r.governor.sent() < 60, "well within the background share: {}", r.governor.sent());
    });
}

#[test]
fn the_socket_answers_pings_while_names_are_looked_up() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.boot().await;
        r.connected().await;
        let c = r.channel("general");
        r.open(&c).await;
        r.fake.fail_next("users/info", slakio_fake::Fault::Hang);
        r.post(&c, "UHANGS0001", "1900000000.000100");
        until("the lookup sent", || r.calls("users/info") >= 1).await;
        let asked = Instant::now();
        r.fake.ping(b"still");
        until("the pong", || r.fake.client_frames().iter().any(|f| f.opcode == 0xA)).await;
        assert!(asked.elapsed() < Duration::from_secs(2), "answered at once: {:?}", asked.elapsed());
        // The message comes once the lookup gave up, with its author's id.
        let Live::Message { users, .. } = r.live().await else { panic!() };
        assert!(users.is_empty());
    });
}

#[test]
fn an_opening_slack_refuses_as_too_many_waits_what_slack_asked() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.fake.rate_limit_sockets(1, "1");
        let start = Instant::now();
        r.boot().await;
        r.lost().await;
        assert!(r.connected().await);
        assert_eq!(r.fake.upgrades().len(), 2);
        assert!(start.elapsed() >= Duration::from_secs(1), "{:?}: Retry-After, not the backoff", start.elapsed());
    });
}

#[test]
fn sockets_that_never_say_hello_keep_the_pause_doubling() {
    rt().block_on(async {
        let quick = Realtime {
            first_retry: Duration::from_millis(40),
            longest_retry: Duration::from_millis(640),
            steady: Duration::from_millis(50),
            silence: Duration::from_millis(120),
            ..QUICK
        };
        let mut r = Run::new(quick).await;
        r.fake.set_hello(false);
        r.boot().await;
        r.lost().await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        // Each socket stays open longer than `steady`, but none said hello: the pause doubles
        // (20–40, 40–80, … ms after each 120 ms of silence) and never starts over.
        let n = r.fake.upgrades().len();
        assert!((3..=8).contains(&n), "{n} openings in 2 s");
        r.quiet(Duration::from_millis(10)).await;
    });
}

#[test]
fn an_opening_the_budget_has_no_room_for_is_said_as_held_once_and_tried_again() {
    rt().block_on(async {
        let limits = Limits { lane_background_per_hour: 0, ..Limits::default() };
        let governor = Governor::new(limits, Box::new(FakeClock::default()), true);
        let mut r = Run::team(0, QUICK, governor).await;
        r.boot().await;
        assert!(matches!(r.live().await, Live::Connection { state: Connection::Held, .. }));
        // Tried again after each pause, without a word more while it stays held.
        tokio::time::sleep(Duration::from_millis(600)).await;
        r.quiet(Duration::from_millis(10)).await;
        assert!(r.fake.upgrades().is_empty(), "nothing went out");
    });
}

#[test]
fn a_machine_that_slept_loses_its_socket_at_once_and_reads_what_it_missed() {
    rt().block_on(async {
        let woke = Realtime {
            wake_check: Duration::from_millis(30),
            wake_gap: Duration::from_secs(2),
            wall: wall_after_sleep,
            ..QUICK
        };
        let mut r = Run::new(woke).await;
        r.boot().await;
        assert!(!r.connected().await);
        // Running on as usual is no sleep.
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(r.fake.upgrades().len(), 1);
        let counts = r.calls("client.counts");
        // The lid was closed an hour; the socket looks open, and Slack would say nothing for 10 s.
        SLEPT.fetch_add(3600, Ordering::SeqCst);
        let at = Instant::now();
        r.lost().await;
        assert!(at.elapsed() < Duration::from_secs(2), "at once, not after the silence: {:?}", at.elapsed());
        until("the close", || r.fake.client_frames().iter().any(|f| f.opcode == 0x8)).await;
        assert!(r.connected().await, "opened again, to catch up");
        assert!(at.elapsed() < Duration::from_secs(3), "without the doubled pause: {:?}", at.elapsed());
        assert_eq!(r.fake.upgrades().len(), 2);
        let Live::Counts { workspace, counts: listed } = r.counted().await else { panic!() };
        assert_eq!(workspace, r.ws().id);
        assert!(!listed.is_empty());
        assert_eq!(r.calls("client.counts"), counts + 1, "read again once");
        r.quiet(Duration::from_millis(300)).await;
        assert_eq!(r.fake.upgrades().len(), 2, "one wake, one reconnect");
    });
}

#[test]
fn wakes_one_after_the_other_do_not_keep_the_pause_short() {
    rt().block_on(async {
        let woke = Realtime {
            first_retry: Duration::from_millis(400),
            longest_retry: Duration::from_secs(10),
            wake_check: Duration::from_millis(30),
            wake_gap: Duration::from_secs(2),
            wall: wall_after_sleeps,
            ..QUICK
        };
        let mut r = Run::new(woke).await;
        r.boot().await;
        assert!(!r.connected().await);
        let mut took = Vec::new();
        for _ in 0..4 {
            SLEPT_AGAIN.fetch_add(3600, Ordering::SeqCst);
            r.lost().await;
            let at = Instant::now();
            assert!(r.connected().await);
            took.push(at.elapsed());
        }
        // The first wake starts the pause over (at most 400 ms); the ones that come within the
        // steady time of it count as failures: the pause doubles (at least 800 ms by the fourth).
        assert!(took[0] < Duration::from_millis(700), "{took:?}");
        assert!(took[3] >= Duration::from_millis(800), "{took:?}");
    });
}

#[test]
fn the_counts_are_read_again_once_after_each_loss_and_never_on_the_first_socket() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.boot().await;
        assert!(!r.connected().await);
        let at_boot = r.calls("client.counts");
        r.quiet(Duration::from_millis(200)).await;
        assert!(r.counts.is_empty() && r.calls("client.counts") == at_boot, "the boot read them");
        for n in 1..=3 {
            r.fake.drop_sockets();
            r.lost().await;
            assert!(r.connected().await);
            r.counted().await;
            assert_eq!(r.calls("client.counts"), at_boot + n);
        }
        // Counts that cannot be read are not pushed: the UI keeps those it has.
        r.fake.fail_next("client.counts", slakio_fake::Fault::Api("internal_error".to_string()));
        r.fake.drop_sockets();
        r.lost().await;
        assert!(r.connected().await);
        until("the counts asked", || r.calls("client.counts") == at_boot + 4).await;
        r.quiet(Duration::from_millis(200)).await;
        assert!(r.counts.is_empty());
    });
}

#[test]
fn switched_off_the_socket_closes_and_nothing_is_sent_switched_on_it_opens_again_and_catches_up() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.boot().await;
        r.connected().await;
        assert!(r.governor.switch(false));
        until("the socket closed", || r.fake.open_sockets() == 0).await;
        let (log, sockets) = (r.fake.log().len(), r.fake.upgrades().len());
        // Nothing is sent: not a boot, nor a page, nor a catch-up.
        let c = r.channel("general");
        let target = Target::Conversation {
            workspace: r.ws().id.clone(),
            conversation: slakio_core::model::ConversationId::new(&c),
        };
        for command in [
            Command::Boot,
            Command::History { target: target.clone(), before: None, limit: 28 },
            Command::History { target: target.clone(), before: Some(Ts(1_900_000_000_000_000)), limit: 28 },
            Command::Catchup { target: target.clone(), limit: 28 },
        ] {
            let Event::Failed { kind, .. } = r.ask(command).await else { panic!() };
            assert_eq!(kind, FailKind::Offline);
        }
        r.quiet(Duration::from_millis(300)).await;
        assert_eq!((r.fake.log().len(), r.fake.upgrades().len()), (log, sockets), "nothing went out");
        // On again: the socket opens, what was missed is read again.
        assert!(r.governor.switch(true));
        assert!(r.connected().await, "to catch up");
        assert_eq!(r.fake.open_sockets(), 1);
        r.counted().await;
        assert!(matches!(r.ask(Command::Catchup { target, limit: 28 }).await, Event::History(_)));
    });
}

#[test]
fn a_network_off_from_the_start_is_never_switched_on() {
    rt().block_on(async {
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), false);
        let mut r = Run::team(0, QUICK, Arc::clone(&governor)).await;
        assert!(!governor.switch(true), "off for the run");
        assert!(!governor.is_on());
        let Event::Failed { kind, .. } = r.ask(Command::Boot).await else { panic!() };
        assert_eq!(kind, FailKind::Offline);
        r.quiet(Duration::from_millis(300)).await;
        assert!(r.fake.upgrades().is_empty() && r.fake.log().is_empty());
    });
}

/// What `command` failed with.
async fn failed(r: &mut Run, command: Command) -> FailKind {
    let Event::Failed { kind, .. } = r.ask(command).await else { panic!("not a failure") };
    kind
}

#[test]
fn a_request_slack_no_longer_signs_in_closes_the_socket_and_nothing_more_is_sent() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.boot().await;
        r.connected().await;
        // Signed out elsewhere; the socket stays open, the next request is refused.
        r.fake.revoke(TOKEN, false);
        let c = r.channel("general");
        let target = Target::Conversation {
            workspace: r.ws().id.clone(),
            conversation: slakio_core::model::ConversationId::new(&c),
        };
        let history = Command::History { target: target.clone(), before: None, limit: 28 };
        assert_eq!(failed(&mut r, history.clone()).await, FailKind::Auth);
        assert!(matches!(r.live().await, Live::Connection { state: Connection::SignedOut, .. }));
        until("the socket closed", || r.fake.open_sockets() == 0).await;
        let (sent, opened) = (r.fake.log().len(), r.fake.upgrades().len());
        // Every later request fails the same way without leaving the machine; no socket opens.
        assert_eq!(failed(&mut r, Command::Boot).await, FailKind::Auth);
        assert_eq!(failed(&mut r, history).await, FailKind::Auth);
        assert_eq!(failed(&mut r, Command::Catchup { target, limit: 200 }).await, FailKind::Auth);
        r.quiet(Duration::from_millis(600)).await;
        assert_eq!((r.fake.log().len(), r.fake.upgrades().len()), (sent, opened), "nothing more went out");
    });
}

#[test]
fn a_socket_closed_and_refused_for_its_sign_in_is_said_once_and_never_opened_again() {
    rt().block_on(async {
        let mut r = Run::new(QUICK).await;
        r.boot().await;
        r.connected().await;
        let sent = r.fake.log().len();
        // Slack ends the session's socket; opening it again is refused for the sign-in.
        r.fake.revoke(TOKEN, true);
        r.lost().await;
        assert!(matches!(r.live().await, Live::Connection { state: Connection::SignedOut, .. }));
        assert_eq!(r.fake.upgrades().len(), 2, "one opening again, refused");
        r.quiet(Duration::from_millis(600)).await;
        assert_eq!(r.fake.upgrades().len(), 2, "never opened again");
        // Requests are refused without going out (the counts after the loss never went either).
        assert_eq!(failed(&mut r, Command::Boot).await, FailKind::Auth);
        assert_eq!(r.fake.log().len(), sent, "{:?}", r.fake.methods());
    });
}
