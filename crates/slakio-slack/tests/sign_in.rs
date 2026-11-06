//! Signing in and opening conversations against the in-process fake Slack: one call to check a
//! sign-in, what an auth failure says, and the limits on opening (a short wait; one open of a
//! conversation under way, the older one cancelled and answered; opens of other conversations,
//! as a layout opened again sends them, all load).

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
        assert!(
            l.path.starts_with("/api/") || l.path.starts_with("/cache/"),
            "only the API and the edge cache, never a file or image host: {}",
            l.path
        );
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
        assert_eq!(fake.methods().len(), 1, "never retried on its own");
        // Checked again with the same client: refused at once, nothing sent.
        assert_eq!(login::check(&client).await.unwrap_err().kind(), FailKind::Auth);
        assert_eq!(fake.methods().len(), 1);
        let (fake, client) = setup().await;
        fake.fail_next("client.userBoot", Fault::Status { status: 503, retry_after: None });
        assert_eq!(login::check(&client).await.unwrap_err().kind(), FailKind::Network);
        assert_eq!(fake.methods().len(), 1, "never retried on its own");
    });
}

/// The first `n` conversations of the first workspace with messages, as targets.
fn conversations(n: usize) -> Vec<Target> {
    let world = World::new(7);
    let ws = world.snapshot().workspaces[0].id.clone();
    (world.snapshot().conversations.iter())
        .filter(|c| c.workspace == ws && world.message_count(&c.id) > 0)
        .take(n)
        .map(|c| Target::Conversation { workspace: ws.clone(), conversation: c.id.clone() })
        .collect()
}

fn open(t: &Target) -> Command {
    Command::History { target: t.clone(), before: None, limit: 28 }
}

#[test]
fn opening_a_conversation_again_quickly_fetches_it_once_and_answers_the_older_opens() {
    rt().block_on(async {
        let (fake, client) = setup().await;
        let target = conversations(1).remove(0);
        let mut handle = slakio_slack::serve(client);
        for i in 1..=3 {
            ask(&handle, i, open(&target));
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
        let views = fake.methods().into_iter().filter(|m| m == "conversations.view").count();
        assert_eq!(views, 1, "one open reached the server");
        only_reads(&fake);
    });
}

/// The panes of a layout opened again ask for their conversations at once: each is answered with
/// its page (none cancels another), one `conversations.view` each, and nothing else but the
/// names of their authors.
fn restored_opens_all_load(n: usize) {
    rt().block_on(async {
        let (fake, client) = setup().await;
        let targets = conversations(n);
        assert_eq!(targets.len(), n);
        let mut handle = slakio_slack::serve(client);
        for (i, t) in targets.iter().enumerate() {
            ask(&handle, i as u64 + 1, open(t));
        }
        let mut answers: Vec<Envelope> = Vec::new();
        for _ in 0..n {
            answers.push(next(&mut handle).await);
        }
        answers.sort_by_key(|a| a.generation);
        for (a, t) in answers.iter().zip(&targets) {
            let Event::History(page) = &a.event else { panic!("{n}: every pane loads: {a:?}") };
            assert_eq!(&page.target, t);
        }
        let methods = fake.methods();
        let views = methods.iter().filter(|m| *m == "conversations.view").count();
        assert_eq!(views, n, "{n}: one open each: {methods:?}");
        // Besides, only the authors' names are looked up.
        let others: Vec<&String> = methods.iter().filter(|m| *m != "conversations.view").collect();
        assert!(others.iter().all(|m| !m.starts_with("conversations.")), "{n}: {methods:?}");
        assert!(methods.len() <= 2 * n, "{n}: bounded: {methods:?}");
        only_reads(&fake);
    });
}

#[test]
fn two_conversations_opened_at_once_both_load() {
    restored_opens_all_load(2);
}

#[test]
fn four_conversations_opened_at_once_all_load() {
    restored_opens_all_load(4);
}

#[test]
fn opening_a_then_b_then_a_again_quickly_loads_both_and_answers_the_first_a() {
    rt().block_on(async {
        let (fake, client) = setup().await;
        let t = conversations(2);
        let mut handle = slakio_slack::serve(client);
        for (i, target) in [&t[0], &t[1], &t[0]].into_iter().enumerate() {
            ask(&handle, i as u64 + 1, open(target));
            tokio::time::sleep(OPEN_DEBOUNCE / 4).await;
        }
        let mut answers: Vec<Envelope> = Vec::new();
        for _ in 0..3 {
            answers.push(next(&mut handle).await);
        }
        answers.sort_by_key(|a| a.generation);
        assert!(matches!(answers[0].event, Event::Failed { kind: FailKind::Busy, .. }), "{:?}", answers[0]);
        assert!(matches!(&answers[1].event, Event::History(p) if p.target == t[1]), "{:?}", answers[1]);
        assert!(matches!(&answers[2].event, Event::History(p) if p.target == t[0]), "{:?}", answers[2]);
        let views = fake.methods().into_iter().filter(|m| m == "conversations.view").count();
        assert_eq!(views, 2, "A once, B once");
    });
}

#[test]
fn a_newer_open_of_a_conversation_cancels_its_open_under_way() {
    rt().block_on(async {
        let (fake, client) = setup().await;
        let a = conversations(1).remove(0);
        let mut handle = slakio_slack::serve(client);
        fake.fail_next("conversations.view", Fault::Hang);
        ask(&handle, 1, open(&a));
        while fake.log().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        ask(&handle, 2, open(&a));
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
        let views = fake.methods().into_iter().filter(|m| m == "conversations.view").count();
        assert_eq!(views, 2);
    });
}

#[test]
fn an_open_under_way_is_never_cancelled_by_another_conversations_open() {
    rt().block_on(async {
        let (fake, client) = setup().await;
        let t = conversations(2);
        let mut handle = slakio_slack::serve(client);
        fake.fail_next("conversations.view", Fault::Hang);
        ask(&handle, 1, open(&t[0]));
        while fake.log().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        ask(&handle, 2, open(&t[1]));
        let second = next(&mut handle).await;
        assert_eq!(second.generation, Generation(2));
        assert!(matches!(second.event, Event::History(_)), "{second:?}");
        // The first one is still on its way: not cancelled, not answered.
        tokio::time::sleep(OPEN_DEBOUNCE * 2).await;
        assert_eq!(fake.hangs_closed(), 0);
        assert!(handle.rx.try_recv().is_err());
    });
}
