//! The user's Activity against the fake Slack: one `activity.feed` a page (the web client's
//! parameters, the cursor of the page before), the texts of its messages looked up with a few
//! `messages.list` (only the conversations it names), their authors named as a page's are;
//! many pages to the end, none at all, hostile answers, Slack asking to slow down, a refused
//! sign-in, a newer page superseding one on its way. Nothing is marked read and nothing else is
//! sent.

use slakio_core::backend::{
    ACTIVITY_PAGE, BackendHandle, Command, Envelope, Event, FailKind, Feed, Generation, NoticeKind, Request,
};
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault};
use slakio_slack::Client;
use slakio_slack::activity::{CHANNELS_PER_CALL, TEXT_CALLS_MAX, TYPES};
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_world::{World, names};
use std::time::Duration;

const TOKEN: &str = concat!("xo", "xc-TEST");

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

struct Run {
    fake: FakeSlack,
    handle: BackendHandle,
    world: World,
    id: u64,
}

impl Run {
    async fn new() -> Self {
        Self::team(0).await
    }

    /// Signed in to the world's `team`-th workspace and booted.
    async fn team(team: usize) -> Self {
        let fake = FakeSlack::start_team(World::demo(), team).await.unwrap();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client =
            Client::new(Credentials::new(TOKEN, "cookie-TEST"), Endpoints::local(fake.addr()).unwrap(), governor);
        let mut r = Self { fake, handle: slakio_slack::serve(client), world: World::demo(), id: 0 };
        assert!(matches!(r.ask(Command::Boot).await, Event::Booted(_)));
        r
    }

    async fn ask(&mut self, command: Command) -> Event {
        self.id += 1;
        self.handle.send(Request { id: Generation(self.id), workspace: None, command }).unwrap();
        let e = next(&mut self.handle).await;
        assert_eq!(e.generation, Generation(self.id));
        e.event
    }

    async fn page(&mut self, cursor: Option<&str>) -> Feed {
        match self.ask(Command::Activity { cursor: cursor.map(str::to_string) }).await {
            Event::Activity(f) => *f,
            other => panic!("{other:?}"),
        }
    }

    /// The fields (decoded) of each request of `method` since the boot.
    fn sent(&self, method: &str) -> Vec<Vec<(String, String)>> {
        (self.fake.log().iter())
            .filter(|l| l.method() == method)
            .map(|l| l.fields().into_iter().map(|(k, v)| (k.to_string(), decode(v))).collect())
            .collect()
    }

    /// The methods asked for after the boot.
    fn after_boot(&self) -> Vec<String> {
        let m = self.fake.methods();
        let boot = m.iter().rposition(|m| m == "dnd.teamInfo").map_or(0, |i| i + 1);
        m[boot..].to_vec()
    }
}

async fn next(h: &mut BackendHandle) -> Envelope {
    tokio::time::timeout(Duration::from_secs(20), h.rx.recv()).await.expect("an answer").unwrap()
}

fn decode(v: &str) -> String {
    let mut out = Vec::new();
    let b = v.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                out.push(u8::from_str_radix(&v[i + 1..i + 3], 16).unwrap());
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8(out).unwrap()
}

fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

#[test]
fn the_newest_page_comes_with_its_texts_and_authors_and_the_exact_params() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let feed = r.page(None).await;
        let ws = r.world.snapshot().workspaces[0].clone();
        let all = r.world.activity(&ws.id);
        assert_eq!(feed.workspace, ws.id);
        assert_eq!((feed.after.as_deref(), feed.next.as_deref()), (None, Some("c20")));
        assert_eq!(feed.notices.len(), ACTIVITY_PAGE as usize);
        for (n, w) in feed.notices.iter().zip(&all) {
            assert_eq!((&n.key, n.at, &n.conversation, n.unread), (&w.key, w.at, &w.conversation, w.unread));
            assert_eq!(n.text.as_ref(), Some(&w.message.text), "{}", n.key);
            match w.kind {
                "message_reaction" => assert_eq!(Some(&n.user), w.reactor.as_ref().map(|(u, _)| u)),
                _ => assert_eq!(n.user, w.message.user, "{}", n.key),
            }
        }
        let feeds = r.sent("activity.feed");
        assert_eq!(feeds.len(), 1);
        let f = &feeds[0];
        let want = [
            ("limit", "20"),
            ("types", TYPES),
            ("mode", "chrono_v1"),
            ("archive_only", "false"),
            ("unread_only", "false"),
            ("priority_only", "false"),
            ("exclude_automations", "false"),
        ];
        for (k, v) in want {
            assert_eq!(field(f, k), Some(v), "{k}");
        }
        assert_eq!(field(f, "cursor"), None);
        // The texts: only the conversations the page names, a few a request.
        let lists = r.sent("messages.list");
        let channels: std::collections::BTreeSet<&str> = feed.notices.iter().map(|n| n.conversation.as_str()).collect();
        assert_eq!(lists.len(), channels.len().div_ceil(CHANNELS_PER_CALL).min(TEXT_CALLS_MAX));
        for l in &lists {
            let ids: serde_json::Value = serde_json::from_str(field(l, "message_ids").unwrap()).unwrap();
            let named = ids.as_array().unwrap();
            assert!(!named.is_empty() && named.len() <= CHANNELS_PER_CALL);
            assert!(named.iter().all(|g| channels.contains(g["channel"].as_str().unwrap())));
            assert_eq!(field(l, "org_wide_aware"), Some("true"));
        }
        let methods = r.after_boot();
        assert!(
            methods.iter().all(|m| ["activity.feed", "messages.list", "users/info"].contains(&m.as_str())),
            "{methods:?}"
        );
        // Every kind the world has is read as its own.
        let kinds: Vec<NoticeKind> = feed.notices.iter().map(|n| n.kind).collect();
        for k in [NoticeKind::Mention, NoticeKind::Thread, NoticeKind::Reaction, NoticeKind::Dm, NoticeKind::Everyone] {
            assert!(kinds.contains(&k), "{k:?} in {kinds:?}");
        }
        let thread = feed.notices.iter().find(|n| n.kind == NoticeKind::Thread).unwrap();
        assert!(thread.thread.is_some_and(|t| t < thread.ts));
    });
}

#[test]
fn pages_follow_their_cursor_to_the_end_each_notice_once() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let ws = r.world.snapshot().workspaces[0].id.clone();
        let total = r.world.activity(&ws).len();
        let mut cursor = None;
        let mut keys = Vec::new();
        let mut pages = 0;
        loop {
            let feed = r.page(cursor.as_deref()).await;
            assert_eq!(feed.after, cursor);
            keys.extend(feed.notices.iter().map(|n| n.key.clone()));
            pages += 1;
            match feed.next {
                Some(next) => cursor = Some(next),
                None => break,
            }
            assert!(pages < 100, "the end comes");
        }
        assert_eq!(keys.len(), total);
        assert_eq!(pages, total.div_ceil(ACTIVITY_PAGE as usize));
        let sent = r.sent("activity.feed");
        assert_eq!(field(&sent[1], "cursor"), Some("c20"));
        // A cursor not of Slack's shape asks nothing.
        let before = r.fake.log().len();
        let odd = r.page(Some("a b\u{1b}[2J")).await;
        assert!(odd.notices.is_empty() && odd.next.is_none());
        assert_eq!(r.fake.log().len(), before);
    });
}

#[test]
fn an_empty_activity_is_one_page_with_nothing_after_it() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.fake.fail_next(
            "activity.feed",
            Fault::Answer(serde_json::json!({"ok": true, "items": [], "response_metadata": {"next_cursor": ""}})),
        );
        let feed = r.page(None).await;
        assert!(feed.notices.is_empty() && feed.next.is_none());
        assert!(r.sent("messages.list").is_empty(), "no text to look up");
    });
}

#[test]
fn a_hostile_answer_is_read_as_remote_text_and_bad_entries_are_left_out() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let ws = r.world.snapshot().workspaces[0].id.clone();
        let hostile = r.world.find(ws.as_str(), names::HOSTILE).unwrap().id.clone();
        let items = serde_json::json!([
            {"key": "h1", "is_unread": true, "feed_ts": "1700000000.000001",
             "item": {"type": "message_reaction", "message": {"ts": "1700000000.000001", "channel": hostile.as_str()},
                      "reaction": {"user": "U\u{1b}[31m", "name": "x\u{1b}]0;owned\u{7}"}}},
            {"key": "h2", "item": {"type": "at_user", "message": {"ts": "1.2.3", "channel": "C0AB"}}},
            {"key": "h3", "item": {"type": "at_user", "message": {"ts": "1700000000.000002", "channel": "../../etc"}}},
            {"key": "h4", "item": "not an object"},
            "not an item",
            {"key": "h5", "item": {"type": "at_user", "message": {"ts": "1700000000.000003", "channel": "C0AB", "author_user_id": "U0ZZ"}}},
            {"key": "h5", "item": {"type": "at_user", "message": {"ts": "1700000000.000004", "channel": "C0AB"}}}
        ]);
        r.fake.fail_next(
            "activity.feed",
            Fault::Answer(serde_json::json!({"ok": true, "items": items, "response_metadata": {"next_cursor": "x y"}})),
        );
        let bodies = serde_json::json!({"ok": true, "messages_data": {
            "C0AB": {"messages": [{"ts": "1700000000.000003", "user": "U0ZZ", "text": "\u{1b}[2Jhi <@U0ZZ>"}]},
            "../x": {"messages": [{"ts": "1700000000.000003", "text": "nope"}]},
            hostile.as_str(): "not a history"
        }});
        r.fake.fail_next("messages.list", Fault::Answer(bodies));
        let feed = r.page(None).await;
        let keys: Vec<&str> = feed.notices.iter().map(|n| n.key.as_str()).collect();
        assert_eq!(keys, ["h1", "h5"], "bad shapes left out, each key once");
        assert_eq!(feed.next, None, "a cursor not of Slack's shape is none");
        let reaction = &feed.notices[0];
        assert!(reaction.user.as_str().is_empty(), "an author not of Slack's shape is unknown");
        assert_eq!(reaction.text, None, "its text could not be read: unknown");
        let name = reaction.reaction.as_ref().unwrap();
        assert!(!name.line().as_str().contains('\u{1b}') && !name.line().as_str().contains('\u{7}'));
        let text = feed.notices[1].text.as_ref().unwrap();
        assert!(text.unsanitized().contains('\u{1b}'), "kept as Slack gave it");
        assert!(!text.line().as_str().contains('\u{1b}'), "never drawn as it came");
        // Not JSON at all: the page fails.
        r.fake.fail_next("activity.feed", Fault::Malformed);
        assert!(matches!(r.ask(Command::Activity { cursor: None }).await, Event::Failed { .. }));
    });
}

#[test]
fn texts_that_cannot_be_read_leave_the_page_without_them() {
    rt().block_on(async {
        let mut r = Run::new().await;
        for _ in 0..TEXT_CALLS_MAX {
            r.fake.fail_next("messages.list", Fault::Status { status: 500, retry_after: None });
        }
        let feed = r.page(None).await;
        assert_eq!(feed.notices.len(), ACTIVITY_PAGE as usize);
        assert!(feed.notices.iter().all(|n| n.text.is_none()), "unknown, not empty");
    });
}

#[test]
fn slack_asking_to_slow_down_fails_the_page_and_nothing_is_sent_during_the_pause() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.fake.fail_next("activity.feed", Fault::Status { status: 429, retry_after: Some("7".into()) });
        let failed = r.ask(Command::Activity { cursor: None }).await;
        assert!(matches!(failed, Event::Failed { kind: FailKind::RateLimited { .. }, .. }), "{failed:?}");
        let paused = r.ask(Command::Activity { cursor: None }).await;
        assert!(matches!(paused, Event::Failed { kind: FailKind::RateLimited { .. }, .. }), "{paused:?}");
        assert_eq!(r.sent("activity.feed").len(), 1, "nothing sent during the pause");
    });
}

#[test]
fn a_refused_sign_in_fails_the_page_for_the_sign_in() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.fake.fail_next("activity.feed", Fault::Api("invalid_auth".into()));
        let failed = r.ask(Command::Activity { cursor: None }).await;
        assert!(matches!(failed, Event::Failed { kind: FailKind::Auth, .. }), "{failed:?}");
        r.fake.fail_next("messages.list", Fault::Api("invalid_auth".into()));
        let failed = r.ask(Command::Activity { cursor: None }).await;
        assert!(matches!(failed, Event::Failed { kind: FailKind::Auth, .. }), "texts refused too: {failed:?}");
    });
}

#[test]
fn the_second_workspace_lists_its_own() {
    rt().block_on(async {
        let mut r = Run::team(1).await;
        let ws = r.world.snapshot().workspaces[1].id.clone();
        let feed = r.page(None).await;
        assert_eq!(feed.workspace, ws);
        let want: Vec<String> = r.world.activity(&ws).iter().map(|n| n.key.clone()).collect();
        let got: Vec<String> = feed.notices.iter().map(|n| n.key.clone()).collect();
        assert_eq!(got, want);
    });
}

#[test]
fn a_newer_page_supersedes_one_on_its_way_and_each_is_answered_once() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.fake.fail_next("activity.feed", Fault::Hang);
        let ask = |id, cursor: Option<&str>| Request {
            id: Generation(id),
            workspace: None,
            command: Command::Activity { cursor: cursor.map(str::to_string) },
        };
        r.handle.send(ask(100, Some("c20"))).unwrap();
        while r.sent("activity.feed").is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        r.handle.send(ask(101, None)).unwrap();
        let first = next(&mut r.handle).await;
        assert_eq!(first.generation, Generation(100));
        assert!(matches!(first.event, Event::Failed { kind: FailKind::Busy, .. }), "{first:?}");
        let second = next(&mut r.handle).await;
        assert_eq!(second.generation, Generation(101));
        assert!(matches!(second.event, Event::Activity(_)), "{second:?}");
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(r.handle.rx.try_recv().is_err(), "nothing answered twice");
    });
}
