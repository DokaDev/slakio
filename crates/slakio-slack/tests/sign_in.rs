//! Signing in and opening conversations against the in-process fake Slack: one call to check a
//! sign-in, what an auth failure says, and the limits on opening (a short wait, one open under
//! way, the older ones cancelled and answered).

use slakio_core::backend::{BackendHandle, Command, Envelope, Event, FailKind, Generation, Request};
use slakio_core::model::Target;
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault};
use slakio_slack::adapter::OPEN_DEBOUNCE;
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_slack::{Client, SlackError, login};
use slakio_world::World;
use std::time::Duration;

/// Synthetic credentials (spelled in parts so no file holds a token-shaped string).
const TOKEN: &str = concat!("xo", "xc-TEST");
const COOKIE: &str = concat!("xo", "xd-TEST%2F1");

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

async fn setup() -> (FakeSlack, Client) {
    let fake = FakeSlack::start(World::new(7)).await.unwrap();
    let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
    let endpoints = Endpoints::local(fake.addr()).unwrap();
    (fake, Client::new(Credentials::new(TOKEN, COOKIE).with_ds("1700000000"), endpoints, governor))
}

async fn next(handle: &mut BackendHandle) -> Envelope {
    tokio::time::timeout(Duration::from_secs(10), handle.rx.recv()).await.expect("an answer").expect("open")
}

fn ask(handle: &BackendHandle, id: u64, command: Command) {
    handle.send(Request { id: Generation(id), workspace: None, command }).unwrap();
}

/// No request of the whole run went anywhere but the workspace API, nor to a saved-items method.
fn only_reads(fake: &FakeSlack) {
    for l in fake.log() {
        assert!(l.path.starts_with("/api/"), "only the API, never a file or image host: {}", l.path);
        assert!(!l.method().starts_with("saved."), "{}", l.method());
    }
}

#[test]
fn a_sign_in_is_checked_with_one_start_call_and_names_the_workspace() {
    rt().block_on(async {
        let (fake, client) = setup().await;
        let world = World::new(7);
        let ws = &world.snapshot().workspaces[0];
        let who = login::check(&client).await.unwrap();
        assert_eq!(who.team, ws.id.as_str());
        assert_eq!(who.user, ws.me.as_str());
        assert!(who.team_name.is_some());
        assert_eq!(fake.methods(), ["client.userBoot"]);
        // The cookies go as the browser sends them; the token only in the body.
        let log = fake.log();
        let cookie = log[0].headers.iter().find(|(n, _)| n.eq_ignore_ascii_case("cookie")).unwrap();
        assert_eq!(cookie.1, format!("d={COOKIE}; d-s=1700000000"));
        assert!(log[0].headers.iter().all(|(_, v)| !v.contains(TOKEN)));
        only_reads(&fake);
    });
}

#[test]
fn a_sign_in_that_slack_refuses_is_an_auth_failure_and_says_nothing_secret() {
    rt().block_on(async {
        let (fake, client) = setup().await;
        fake.fail_next("client.userBoot", Fault::Api("invalid_auth".into()));
        let e = login::check(&client).await.unwrap_err();
        assert_eq!(e, SlackError::Auth { code: "invalid_auth".into() });
        assert_eq!(e.kind(), FailKind::Auth);
        let said = format!("{e:?} {}", e.detail());
        assert!(!said.contains(TOKEN) && !said.contains(COOKIE), "{said}");
        fake.fail_next("client.userBoot", Fault::Status { status: 503, retry_after: None });
        assert_eq!(login::check(&client).await.unwrap_err().kind(), FailKind::Network);
        assert_eq!(fake.methods().len(), 2, "never retried on its own");
    });
}

#[test]
fn opening_quickly_fetches_only_the_last_conversation_and_answers_the_others() {
    rt().block_on(async {
        let (fake, client) = setup().await;
        let world = World::new(7);
        let ws = world.snapshot().workspaces[0].id.clone();
        let targets: Vec<Target> = world
            .snapshot()
            .conversations
            .iter()
            .filter(|c| c.workspace == ws && world.message_count(&c.id) > 0)
            .take(3)
            .map(|c| Target::Conversation { workspace: ws.clone(), conversation: c.id.clone() })
            .collect();
        let mut handle = slakio_slack::serve(client);
        for (i, t) in targets.iter().enumerate() {
            ask(&handle, i as u64 + 1, Command::History { target: t.clone(), before: None, limit: 28 });
            tokio::time::sleep(OPEN_DEBOUNCE / 4).await;
        }
        let mut answers: Vec<Envelope> = Vec::new();
        for _ in 0..3 {
            answers.push(next(&mut handle).await);
        }
        answers.sort_by_key(|a| a.generation);
        for older in &answers[..2] {
            assert!(matches!(older.event, Event::Failed { kind: FailKind::Busy, .. }), "{older:?}");
        }
        assert!(matches!(answers[2].event, Event::History(_)), "{:?}", answers[2]);
        assert_eq!(fake.methods(), ["conversations.view"], "one open reached the server");
        only_reads(&fake);
    });
}

#[test]
fn a_newer_open_cancels_one_under_way_so_one_view_at_most_is_in_flight() {
    rt().block_on(async {
        let (fake, client) = setup().await;
        let world = World::new(7);
        let ws = world.snapshot().workspaces[0].id.clone();
        let mut ids = world.snapshot().conversations.iter().filter(|c| c.workspace == ws).map(|c| c.id.clone());
        let (a, b) = (ids.next().unwrap(), ids.next().unwrap());
        let mut handle = slakio_slack::serve(client);
        fake.fail_next("conversations.view", Fault::Hang);
        ask(
            &handle,
            1,
            Command::History {
                target: Target::Conversation { workspace: ws.clone(), conversation: a },
                before: None,
                limit: 28,
            },
        );
        while fake.log().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        ask(
            &handle,
            2,
            Command::History {
                target: Target::Conversation { workspace: ws, conversation: b },
                before: None,
                limit: 28,
            },
        );
        let first = next(&mut handle).await;
        assert_eq!(first.generation, Generation(1));
        assert!(matches!(first.event, Event::Failed { kind: FailKind::Busy, .. }), "{first:?}");
        // The hung request's connection is closed before the next one is sent.
        tokio::time::timeout(Duration::from_secs(2), async {
            while fake.hangs_closed() == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the older open was cancelled");
        let second = next(&mut handle).await;
        assert_eq!(second.generation, Generation(2));
        assert!(matches!(second.event, Event::History(_)));
        assert_eq!(fake.methods(), ["conversations.view", "conversations.view"]);
    });
}
