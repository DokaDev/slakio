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
