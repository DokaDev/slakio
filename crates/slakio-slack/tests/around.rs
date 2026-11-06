//! A conversation read around a message (a Slack link opened), against the fake Slack: two
//! `conversations.history` requests, the message and those before it (`latest`, `inclusive`),
//! then those after it (`oldest`); the newer ones join only when Slack says none is left past
//! them, else the page says its newest are not read (no gap hidden). A refusal (not in the
//! channel) fails it as Slack's own error. Nothing but these two requests is sent.

use slakio_core::backend::{BackendHandle, Command, Envelope, Event, FailKind, Generation, Page, Request};
use slakio_core::model::{Target, Ts};
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault};
use slakio_slack::Client;
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
        let fake = FakeSlack::start(World::demo()).await.unwrap();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client =
            Client::new(Credentials::new(TOKEN, "cookie-TEST"), Endpoints::local(fake.addr()).unwrap(), governor);
        Self { fake, handle: slakio_slack::serve(client), world: World::demo(), id: 0 }
    }

    async fn ask(&mut self, command: Command) -> Event {
        self.id += 1;
        self.handle.send(Request { id: Generation(self.id), workspace: None, command }).unwrap();
        let e: Envelope =
            tokio::time::timeout(Duration::from_secs(20), self.handle.rx.recv()).await.expect("an answer").unwrap();
        assert_eq!(e.generation, Generation(self.id));
        e.event
    }

    async fn around(&mut self, target: &Target, at: Ts) -> Page {
        match self.ask(Command::Around { target: target.clone(), at, limit: 28 }).await {
            Event::History(p) => *p,
            other => panic!("{other:?}"),
        }
    }

    fn target(&self, name: &str) -> Target {
        let s = self.world.snapshot();
        let c = s.conversations.iter().find(|c| c.workspace == s.workspaces[0].id && c.name == name).unwrap();
        Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() }
    }

    /// The fields of every `conversations.history` request, in order.
    fn histories(&self) -> Vec<Vec<(String, String)>> {
        (self.fake.log().iter())
            .filter(|l| l.method() == "conversations.history")
            .map(|l| l.fields().into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
            .collect()
    }
}

async fn next(h: &mut BackendHandle) -> Envelope {
    tokio::time::timeout(Duration::from_secs(20), h.rx.recv()).await.expect("an answer").unwrap()
}

fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

#[test]
fn an_old_message_is_read_with_those_before_it_and_the_page_says_the_newest_are_not_read() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let big = r.target(names::BIG_HISTORY);
        let at = r.world.message(big.conversation(), 1000).unwrap().ts;
        let page = r.around(&big, at).await;
        let want: Vec<Ts> = (973..=1000).filter_map(|i| r.world.message(big.conversation(), i)).map(|m| m.ts).collect();
        assert_eq!(page.messages.iter().map(|m| m.ts).collect::<Vec<_>>(), want);
        assert!(page.cut && !page.complete);
        let asked = r.histories();
        assert_eq!(asked.len(), 2, "{asked:?}");
        assert_eq!(field(&asked[0], "latest"), Some(at.to_string().as_str()));
        assert_eq!(field(&asked[0], "inclusive"), Some("true"));
        assert_eq!(field(&asked[1], "oldest"), Some(at.to_string().as_str()));
        assert_eq!(field(&asked[1], "inclusive"), None);
        assert_eq!(r.fake.methods().iter().filter(|m| !m.starts_with("conversations.history")).count(), 0);
    });
}

#[test]
fn a_message_near_the_newest_is_read_through_to_the_newest() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let big = r.target(names::BIG_HISTORY);
        let n = r.world.message_count(big.conversation());
        let at = r.world.message(big.conversation(), n - 10).unwrap().ts;
        let page = r.around(&big, at).await;
        assert!(!page.cut, "it reaches the newest");
        let last = r.world.message(big.conversation(), n - 1).unwrap().ts;
        assert_eq!(page.messages.last().map(|m| m.ts), Some(last));
        assert!(page.messages.iter().any(|m| m.ts == at));
        let ts: Vec<Ts> = page.messages.iter().map(|m| m.ts).collect();
        assert!(ts.windows(2).all(|w| w[0] < w[1]), "in order, each once");
        // The first message of a conversation: its start is in.
        let first = r.world.message(big.conversation(), 0).unwrap().ts;
        assert!(r.around(&big, first).await.complete);
    });
}

#[test]
fn a_channel_slack_refuses_fails_as_its_own_error() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let big = r.target(names::BIG_HISTORY);
        r.fake.fail_next("conversations.history", Fault::Api("not_in_channel".into()));
        let failed = r.ask(Command::Around { target: big, at: Ts(1_700_000_000_000_000), limit: 28 }).await;
        let Event::Failed { kind, .. } = failed else { panic!("{failed:?}") };
        assert_eq!(kind, FailKind::Api { code: "not_in_channel".into() });
    });
}

#[test]
fn an_answer_that_does_not_say_whether_more_is_left_is_neither_complete_nor_through_to_the_newest() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let big = r.target(names::BIG_HISTORY);
        let m = r.world.message(big.conversation(), 1000).unwrap();
        let ts = format!("{}.{:06}", m.ts.0 / 1_000_000, m.ts.0 % 1_000_000);
        let one = serde_json::json!({"ok": true, "messages": [{"type": "message", "ts": ts, "user": m.user.as_str(), "text": "x"}]});
        r.fake.fail_next("conversations.history", Fault::Answer(one));
        r.fake.fail_next("conversations.history", Fault::Answer(serde_json::json!({"ok": true, "messages": []})));
        let page = r.around(&big, m.ts).await;
        assert_eq!(page.messages.iter().map(|m| m.ts).collect::<Vec<_>>(), [m.ts]);
        assert!(page.cut && !page.complete, "unknown is neither: {page:?}");
    });
}

#[test]
fn when_the_newer_ones_cannot_be_read_the_message_and_those_before_it_are_a_cut_page() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let big = r.target(names::BIG_HISTORY);
        let n = r.world.message_count(big.conversation());
        // Near the newest, where the newer ones would have joined.
        let at = r.world.message(big.conversation(), n - 10).unwrap().ts;
        r.fake.fail_next("conversations.history", Fault::Pass);
        r.fake.fail_next("conversations.history", Fault::Status { status: 503, retry_after: None });
        let page = r.around(&big, at).await;
        assert!(page.cut, "the newest are not read");
        assert_eq!(page.messages.last().map(|m| m.ts), Some(at));
        assert_eq!(page.messages.len(), 28);
        assert_eq!(r.histories().len(), 2);
        // The sign-in refused fails it as ever.
        r.fake.fail_next("conversations.history", Fault::Pass);
        r.fake.fail_next("conversations.history", Fault::Api("invalid_auth".into()));
        let failed = r.ask(Command::Around { target: big, at, limit: 28 }).await;
        assert!(matches!(failed, Event::Failed { kind: FailKind::Auth, .. }), "{failed:?}");
    });
}

#[test]
fn a_page_of_the_same_conversation_supersedes_a_read_around_on_its_way_and_each_is_answered_once() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let big = r.target(names::BIG_HISTORY);
        let at = r.world.message(big.conversation(), 1000).unwrap().ts;
        r.fake.fail_next("conversations.history", Fault::Hang);
        r.handle
            .send(Request {
                id: Generation(1),
                workspace: None,
                command: Command::Around { target: big.clone(), at, limit: 28 },
            })
            .unwrap();
        while r.fake.log().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let open = Command::History { target: big.clone(), before: None, limit: 28 };
        r.handle.send(Request { id: Generation(2), workspace: None, command: open }).unwrap();
        let first = next(&mut r.handle).await;
        assert_eq!(first.generation, Generation(1));
        assert!(matches!(first.event, Event::Failed { kind: FailKind::Busy, .. }), "{first:?}");
        let second = next(&mut r.handle).await;
        assert_eq!(second.generation, Generation(2));
        assert!(matches!(second.event, Event::History(_)), "{second:?}");
        // And the other way round: a read around supersedes an open on its way.
        r.handle
            .send(Request {
                id: Generation(3),
                workspace: None,
                command: Command::History { target: big.clone(), before: None, limit: 28 },
            })
            .unwrap();
        r.handle
            .send(Request {
                id: Generation(4),
                workspace: None,
                command: Command::Around { target: big, at, limit: 28 },
            })
            .unwrap();
        let third = next(&mut r.handle).await;
        assert_eq!(third.generation, Generation(3));
        assert!(matches!(third.event, Event::Failed { kind: FailKind::Busy, .. }), "{third:?}");
        let fourth = next(&mut r.handle).await;
        assert_eq!(fourth.generation, Generation(4));
        assert!(matches!(fourth.event, Event::History(_)), "{fourth:?}");
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(r.handle.rx.try_recv().is_err(), "nothing answered twice");
    });
}
