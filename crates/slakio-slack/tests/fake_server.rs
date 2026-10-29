//! The transport and the backend against the in-process fake Slack: what goes over the wire,
//! what is refused before it, and what the UI hears back.

use slakio_core::backend::{BackendHandle, Command, Envelope, Event, FailKind, Generation, Request, SavedChange};
use slakio_core::model::{ConversationKind, SavedId, Target};
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault};
use slakio_slack::governor::{FakeClock, Governor, Limits, Priority};
use slakio_slack::request::Endpoints;
use slakio_slack::{Client, SlackError};
use slakio_world::World;
use std::sync::Arc;
use std::time::Duration;

/// A synthetic token (spelled in two parts so no file holds a token-shaped string).
const TOKEN: &str = concat!("xo", "xc-TEST");

/// The header names of the web client's API requests, in its order.
const HEADER_ORDER: &[&str] = &[
    "host",
    "user-agent",
    "accept",
    "accept-language",
    "origin",
    "sec-fetch-site",
    "sec-fetch-mode",
    "sec-fetch-dest",
    "sec-ch-ua",
    "sec-ch-ua-mobile",
    "sec-ch-ua-platform",
    "cache-control",
    "pragma",
    "priority",
    "content-type",
    "content-length",
    "cookie",
];

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

async fn setup(limits: Limits, network: bool) -> (FakeSlack, Client, Arc<Governor>) {
    let fake = FakeSlack::start(World::new(7)).await.unwrap();
    let governor = Governor::new(limits, Box::new(FakeClock::default()), network);
    let endpoints = Endpoints::local(fake.addr()).unwrap();
    let client = Client::new(Credentials::new(TOKEN, "cookie-TEST"), endpoints, Arc::clone(&governor));
    (fake, client, governor)
}

async fn next(handle: &mut BackendHandle) -> Envelope {
    tokio::time::timeout(Duration::from_secs(10), handle.rx.recv()).await.expect("an answer").expect("open")
}

fn ask(handle: &BackendHandle, id: u64, command: Command) {
    handle.send(Request { id: Generation(id), workspace: None, command }).unwrap();
}

#[test]
fn the_wire_carries_the_headers_in_order_and_the_token_in_the_body() {
    rt().block_on(async {
        let (fake, client, _) = setup(Limits::default(), true).await;
        let _: serde_json::Value = client.call_named("client.counts", &[], Priority::User).await.unwrap();
        let log = fake.log();
        assert_eq!(log.len(), 1);
        let names: Vec<String> = log[0].headers.iter().map(|(n, _)| n.to_ascii_lowercase()).collect();
        assert_eq!(names, HEADER_ORDER);
        assert_eq!(log[0].fields()[0], ("token", TOKEN));
        assert!(log[0].headers.iter().all(|(_, v)| !v.contains(TOKEN)), "never a header");
        assert!(log[0].query.starts_with("_x_id=noversion-"), "{}", log[0].query);
        assert!(log[0].query.ends_with("&fp=6e&_x_num_retries=0"));
    });
}

#[test]
fn a_method_off_the_allowlist_is_refused_before_any_io() {
    rt().block_on(async {
        let (fake, client, governor) = setup(Limits::default(), true).await;
        for name in ["chat.postMessage", "conversations.mark", "saved.list", "users.list", "conversations.list"] {
            let e = client.call_named(name, &[("channel", "C1")], Priority::User).await.unwrap_err();
            assert!(matches!(e, SlackError::NotAllowed { .. }), "{name}: {e:?}");
            assert_eq!(e.kind(), FailKind::ReadOnly);
        }
        assert!(fake.log().is_empty(), "nothing reached the server");
        assert_eq!(governor.sent(), 0, "nothing was counted");
    });
}

#[test]
fn with_the_network_off_nothing_reaches_the_server() {
    rt().block_on(async {
        let (fake, client, _) = setup(Limits::default(), false).await;
        let e = client.call_named("client.counts", &[], Priority::User).await.unwrap_err();
        assert_eq!(e.kind(), FailKind::Offline);
        assert!(fake.log().is_empty());
    });
}

#[test]
fn switching_the_network_off_ends_a_request_in_flight() {
    rt().block_on(async {
        let (fake, client, governor) = setup(Limits::default(), true).await;
        fake.fail_next("client.counts", Fault::Hang);
        let call = client.call_named("client.counts", &[], Priority::User);
        let stop = async {
            while fake.log().is_empty() {
                tokio::task::yield_now().await;
            }
            governor.stop();
        };
        let (answer, ()) = tokio::join!(call, stop);
        assert_eq!(answer.unwrap_err().kind(), FailKind::Offline);
        // The connection is closed at once, not left to a task until the server answers.
        tokio::time::timeout(Duration::from_secs(2), async {
            while fake.hangs_closed() == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the hung connection was closed");
        assert!(client.call_named("client.counts", &[], Priority::User).await.is_err());
        assert_eq!(fake.log().len(), 1);
    });
}

#[test]
fn the_cap_stops_requests_before_the_server() {
    rt().block_on(async {
        let (fake, client, _) =
            setup(Limits { per_minute: 2, background_per_minute: 1, ..Limits::default() }, true).await;
        for _ in 0..2 {
            client.call_named("client.counts", &[], Priority::User).await.unwrap();
        }
        let e = client.call_named("client.counts", &[], Priority::User).await.unwrap_err();
        assert_eq!(e.kind(), FailKind::CapReached);
        assert_eq!(fake.log().len(), 2);
    });
}

#[test]
fn slacks_errors_reach_the_ui_as_failure_kinds() {
    rt().block_on(async {
        let (fake, client, _) = setup(Limits::default(), true).await;
        let cases = [
            (Fault::Api("channel_not_found".into()), FailKind::Api { code: "channel_not_found".into() }),
            (Fault::Status { status: 502, retry_after: None }, FailKind::Network),
            (Fault::Malformed, FailKind::Network),
            // Last: after it the method waits its Retry-After.
            (
                Fault::Status { status: 429, retry_after: Some("7".into()) },
                FailKind::RateLimited { retry_after: Some(Duration::from_secs(7)) },
            ),
        ];
        for (fault, kind) in cases {
            fake.fail_next("client.counts", fault.clone());
            let e = client.call_named("client.counts", &[], Priority::User).await.unwrap_err();
            assert_eq!(e.kind(), kind, "{fault:?}");
        }
        // A sign-in Slack refuses: said as such, and the client sends nothing more after it.
        let (fake, client, _) = setup(Limits::default(), true).await;
        fake.fail_next("client.counts", Fault::Api("invalid_auth".into()));
        let e = client.call_named("client.counts", &[], Priority::User).await.unwrap_err();
        assert_eq!(e.kind(), FailKind::Auth);
        let e = client.call_named("client.counts", &[], Priority::User).await.unwrap_err();
        assert_eq!(e.kind(), FailKind::Auth);
        assert_eq!(fake.log().len(), 1, "nothing sent after the refusal");
    });
}

#[test]
fn the_backend_boots_from_the_start_bundle_and_reads_a_conversation() {
    rt().block_on(async {
        let (fake, client, _) = setup(Limits::default(), true).await;
        let world = World::new(7);
        let ws = world.snapshot().workspaces[0].clone();
        let mut handle = slakio_slack::serve(client);
        assert!(handle.capabilities.read_only && !handle.capabilities.demo);
        ask(&handle, 1, Command::Boot);
        let Event::Booted(snapshot) = next(&mut handle).await.event else { panic!("booted") };
        assert_eq!(
            fake.methods(),
            ["client.userBoot", "client.counts", "users/info", "dnd.teamInfo"],
            "the DMs' people by name, and whether they are in do not disturb"
        );
        assert_eq!(snapshot.workspaces[0].id, ws.id);
        assert_eq!(snapshot.saved, None, "saved items are unknown, not none");
        let expected = world
            .snapshot()
            .conversations
            .iter()
            .filter(|c| c.workspace == ws.id && !matches!(c.kind, ConversationKind::GroupDm { .. }))
            .count();
        assert_eq!(snapshot.conversations.len(), expected);
        // The second request is signed in: it carries the team.
        assert!(fake.log()[1].query.contains(&format!("slack_route={}", ws.id)));

        let channel = world
            .snapshot()
            .conversations
            .iter()
            .find(|c| c.workspace == ws.id && world.message_count(&c.id) > 30)
            .unwrap();
        let target = Target::Conversation { workspace: ws.id.clone(), conversation: channel.id.clone() };
        ask(&handle, 2, Command::History { target: target.clone(), before: None, limit: 50 });
        let answer = next(&mut handle).await;
        assert_eq!(answer.generation, Generation(2));
        let Event::History(page) = answer.event else { panic!("{answer:?}") };
        let (expected, _) = world.history(&channel.id, None, 28);
        assert_eq!(
            page.messages.iter().map(|m| m.ts).collect::<Vec<_>>(),
            expected.iter().map(|m| m.ts).collect::<Vec<_>>()
        );
        assert!(!page.complete);
        let view = fake.log().into_iter().rfind(|l| l.method() == "conversations.view").unwrap();
        assert_eq!(view.method(), "conversations.view");
        assert!(view.fields().contains(&("count", "28")), "a page is at most 28");
    });
}

#[test]
fn a_change_is_refused_without_a_request_and_a_failure_is_answered() {
    rt().block_on(async {
        let (fake, client, _) = setup(Limits::default(), true).await;
        let mut raw = slakio_slack::serve(client);
        let save = Request {
            id: Generation(1),
            workspace: None,
            command: Command::Save { item: SavedId::new("S1"), change: SavedChange::Complete },
        };
        assert!(raw.send(save.clone()).is_err(), "the handle refuses it first");
        // Past the handle's own refusal, the backend refuses it too.
        raw.tx.send(save).await.unwrap();
        let answer = next(&mut raw).await;
        assert!(matches!(answer.event, Event::Failed { kind: FailKind::ReadOnly, .. }));
        assert!(fake.log().is_empty());

        fake.fail_next("client.userBoot", Fault::Api("invalid_auth".into()));
        ask(&raw, 2, Command::Boot);
        let answer = next(&mut raw).await;
        assert!(
            matches!(answer.event, Event::Failed { request: Generation(2), kind: FailKind::Auth, .. }),
            "{answer:?}"
        );
        assert_eq!(fake.methods(), ["client.userBoot"], "no counts without a start");
    });
}

#[test]
fn a_newer_page_of_the_same_target_supersedes_one_in_flight() {
    rt().block_on(async {
        let (fake, client, _) = setup(Limits::default(), true).await;
        let world = World::new(7);
        let ws = world.snapshot().workspaces[0].id.clone();
        let channel = world
            .snapshot()
            .conversations
            .iter()
            .find(|c| c.workspace == ws && world.message_count(&c.id) > 0)
            .unwrap();
        let target = Target::Conversation { workspace: ws, conversation: channel.id.clone() };
        let mut handle = slakio_slack::serve(client);
        fake.fail_next("conversations.view", Fault::Hang);
        ask(&handle, 1, Command::History { target: target.clone(), before: None, limit: 28 });
        while fake.log().is_empty() {
            tokio::task::yield_now().await;
        }
        ask(&handle, 2, Command::History { target, before: None, limit: 28 });
        let answer = next(&mut handle).await;
        assert_eq!(answer.generation, Generation(2));
        assert!(matches!(answer.event, Event::History(_)));
        assert!(
            tokio::time::timeout(Duration::from_millis(200), handle.rx.recv()).await.is_err(),
            "the older one is dropped"
        );
    });
}

#[test]
fn after_a_429_the_method_is_not_sent_again_until_retry_after() {
    rt().block_on(async {
        let (fake, client, _) = setup(Limits::default(), true).await;
        fake.fail_next("client.counts", Fault::Status { status: 429, retry_after: Some("30".into()) });
        assert!(client.call_named("client.counts", &[], Priority::User).await.is_err());
        let e = client.call_named("client.counts", &[], Priority::User).await.unwrap_err();
        assert!(matches!(e, SlackError::RateLimited { retry_after: Some(_) }), "{e:?}");
        assert_eq!(fake.methods(), ["client.counts"], "the second never reached the server");
        // Another method goes on.
        let _: serde_json::Value = client.call_named("client.userBoot", &[], Priority::User).await.unwrap();
    });
}
