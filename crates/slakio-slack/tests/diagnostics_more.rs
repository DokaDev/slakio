//! More of the picture diagnostics against the fake Slack: the words of failures (the network,
//! the governor's shares), the lookups of the user's own photo, the sidebar probe that never
//! signs out, and caps on what an answer's shape may make the log write.

use serde_json::json;
use slakio_core::backend::{BackendHandle, Command, Envelope, Event, Generation, Request};
use slakio_core::diag::Diag;
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault};
use slakio_slack::Client;
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_world::World;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const TOKEN: &str = concat!("xo", "xc-TEST");

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-diag-more-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("state").join("pictures.log")
}

struct Run {
    fake: FakeSlack,
    handle: BackendHandle,
    diag: Diag,
    path: PathBuf,
    id: u64,
}

impl Run {
    /// The fake Slack, diagnostics on; requests go to `to` (the fake's own address when `None`).
    async fn new(name: &str, to: Option<SocketAddr>) -> Self {
        let path = scratch(name);
        let diag = Diag::to(path.clone());
        let fake = FakeSlack::start(World::demo()).await.unwrap();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let endpoints = Endpoints::local(to.unwrap_or(fake.addr())).unwrap();
        let client = Client::new(Credentials::new(TOKEN, "cookie-TEST"), endpoints, Arc::clone(&governor))
            .with_diag(diag.clone());
        Self { fake, handle: slakio_slack::serve(client), diag, path, id: 0 }
    }

    async fn ask(&mut self, command: Command) -> Event {
        self.id += 1;
        self.handle.send(Request { id: Generation(self.id), workspace: None, command }).unwrap();
        let e: Envelope =
            tokio::time::timeout(Duration::from_secs(20), self.handle.rx.recv()).await.expect("an answer").unwrap();
        e.event
    }

    fn calls(&self, method: &str) -> usize {
        self.fake.log().iter().filter(|l| l.method() == method).count()
    }

    /// What the file holds now.
    fn text(&self) -> String {
        self.diag.flush();
        std::fs::read_to_string(&self.path).unwrap_or_default()
    }
}

fn has(text: &str, line: &str) {
    assert!(text.contains(line), "no `{line}` in\n{text}");
}

#[test]
fn a_host_that_cannot_be_reached_is_the_network() {
    rt().block_on(async {
        // An address nothing listens on: the connection is refused.
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let to = closed.local_addr().unwrap();
        drop(closed);
        let mut r = Run::new("network", Some(to)).await;
        assert!(matches!(r.ask(Command::Emoji).await, Event::Failed { .. }));
        let text = r.text();
        has(&text, "emoji-list failed network");
        assert!(!text.contains("failed other"), "{text}");
    });
}

#[test]
fn the_user_s_own_lookup_says_what_it_gave_and_the_answer_s_counts() {
    rt().block_on(async {
        let mut r = Run::new("lookup", None).await;
        let me = World::demo().snapshot().workspaces[0].me.as_str().to_string();
        r.fake.fail_next("users/info", Fault::Answer(json!({"ok": true, "results": [], "pending_ids": [me]})));
        assert!(matches!(r.ask(Command::Boot).await, Event::Booted(_)));
        let text = r.text();
        has(&text, "self-photo lookup result standing=unknown in_results=no photo=none");
        has(&text, "avatar counted lookup results=0 pending_ids=1 failed_ids=-");
        has(&text, "avatar field lookup_key name=pending_ids");
        has(&text, "self-photo source none");
        assert!(r.calls("users/info") >= 1);
    });
}
