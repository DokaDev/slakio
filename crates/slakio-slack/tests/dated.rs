//! A conversation read from a date against the fake Slack: the newest after the date first;
//! when they are all there is, with those before it (through to the newest); else a few pages
//! that end where the first message after the date likely is, until one holds it. Only
//! `conversations.history` (and name lookups) is asked, a few times at most.

use slakio_core::backend::{BackendHandle, Command, Event, Generation, Page, Request};
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

    async fn dated(&mut self, target: &Target, at: Ts) -> Page {
        self.id += 1;
        let command = Command::Dated { target: target.clone(), at, limit: 28 };
        self.handle.send(Request { id: Generation(self.id), workspace: None, command }).unwrap();
        let e = tokio::time::timeout(Duration::from_secs(20), self.handle.rx.recv()).await.expect("answer").unwrap();
        assert_eq!(e.generation, Generation(self.id));
        match e.event {
            Event::History(p) => *p,
            other => panic!("{other:?}"),
        }
    }

    fn target(&self, name: &str) -> Target {
        let s = self.world.snapshot();
        let c = s.conversations.iter().find(|c| c.workspace == s.workspaces[0].id && c.name == name).unwrap();
        Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() }
    }

    fn histories(&self) -> usize {
        self.fake.methods().iter().filter(|m| *m == "conversations.history").count()
    }
}

fn ts(page: &Page) -> Vec<Ts> {
    page.messages.iter().map(|m| m.ts).collect()
}

#[test]
fn a_date_deep_in_a_long_conversation_lands_on_its_first_message_in_a_few_requests() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let big = r.target(names::BIG_HISTORY);
        for index in [5000, 120, 9900 - 200] {
            let before = r.histories();
            let first = r.world.message(big.conversation(), index).unwrap().ts;
            // Just after the message before it: the first on or after is `index`.
            let prev = r.world.message(big.conversation(), index - 1).unwrap().ts;
            let at = Ts(prev.0 + 1);
            let page = r.dated(&big, at).await;
            assert!(ts(&page).contains(&first), "{index}: the first after the date is read");
            assert!(ts(&page).iter().any(|t| *t < at), "{index}: with some before it");
            assert!(page.cut && !page.complete);
            let ts = ts(&page);
            assert!(ts.windows(2).all(|w| w[0] < w[1]));
            let asked = r.histories() - before;
            assert!((2..=7).contains(&asked), "{index}: {asked} requests");
        }
        assert!(r.fake.methods().iter().all(|m| m == "conversations.history" || m == "users/info"));
    });
}

#[test]
fn a_recent_date_reads_through_to_the_newest_and_a_future_one_reads_the_newest() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let big = r.target(names::BIG_HISTORY);
        let n = r.world.message_count(big.conversation());
        let first = r.world.message(big.conversation(), n - 5).unwrap().ts;
        let page = r.dated(&big, first).await;
        assert!(!page.cut, "all after it are read");
        assert!(ts(&page).contains(&first));
        assert_eq!(ts(&page).last(), r.world.message(big.conversation(), n - 1).map(|m| m.ts).as_ref());
        assert_eq!(r.histories(), 2);
        let later = Ts(r.world.message(big.conversation(), n - 1).unwrap().ts.0 + 86_400_000_000);
        let newest = r.dated(&big, later).await;
        assert!(!newest.cut);
        assert_eq!(ts(&newest).last(), r.world.message(big.conversation(), n - 1).map(|m| m.ts).as_ref());
        // Long before the first message: the oldest found after it, in a few requests (reading
        // back from there reaches the start).
        let before = r.histories();
        let start = r.dated(&big, Ts(1)).await;
        assert!(start.cut && !start.messages.is_empty());
        assert!(r.histories() - before <= 7);
        // Just before it: its first message, and the start.
        let first = r.world.message(big.conversation(), 0).unwrap().ts;
        let start = r.dated(&big, Ts(first.0 - 1)).await;
        assert!(start.complete && ts(&start).first() == Some(&first), "{:?}", ts(&start).first());
    });
}

#[test]
fn when_the_pages_tried_miss_it_the_oldest_after_the_date_is_shown_and_a_failure_fails_it() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let big = r.target(names::BIG_HISTORY);
        let at = r.world.message(big.conversation(), 3000).unwrap().ts;
        // Every page tried is empty (as if nothing were there): the page ending at the oldest
        // after the date is read last.
        r.fake.fail_next("conversations.history", Fault::Pass);
        for _ in 0..4 {
            r.fake.fail_next(
                "conversations.history",
                Fault::Answer(serde_json::json!({"ok": true, "messages": [], "has_more": true})),
            );
        }
        let page = r.dated(&big, at).await;
        assert!(page.cut && !page.messages.is_empty());
        assert!(ts(&page).iter().all(|t| *t >= at), "the oldest found after the date, read back from");
        assert_eq!(r.histories(), 7);
        r.fake.fail_next("conversations.history", Fault::Api("not_in_channel".into()));
        r.id += 1;
        let command = Command::Dated { target: big, at, limit: 28 };
        r.handle.send(Request { id: Generation(r.id), workspace: None, command }).unwrap();
        let e = tokio::time::timeout(Duration::from_secs(20), r.handle.rx.recv()).await.unwrap().unwrap();
        assert!(matches!(e.event, Event::Failed { .. }), "{e:?}");
    });
}

#[test]
fn an_answer_holding_a_message_older_than_the_date_is_read_without_harm() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let big = r.target(names::BIG_HISTORY);
        let at = r.world.message(big.conversation(), 3000).unwrap().ts;
        let old = r.world.message(big.conversation(), 10).unwrap().ts;
        // Asked for what came after the date, Slack answers with one message from before it.
        let stamp = format!("{}.{:06}", old.0 / 1_000_000, old.0 % 1_000_000);
        let answer = serde_json::json!({"ok": true, "has_more": true,
            "messages": [{"type": "message", "ts": stamp, "user": "UDEMOA001", "text": "older"}]});
        r.fake.fail_next("conversations.history", Fault::Answer(answer));
        let page = r.dated(&big, at).await;
        assert!(page.cut, "{:?}", ts(&page));
        assert!(r.histories() <= 7);
    });
}
