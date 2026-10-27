//! People by name against the in-process fake Slack: the DMs' people at the start and a page's
//! authors, looked up by id in batches (never a directory listing), each id once a session.

use slakio_core::backend::{BackendHandle, Command, Envelope, Event, Generation, Request};
use slakio_core::model::{ConversationKind, Target};
use slakio_core::secret::Credentials;
use slakio_fake::FakeSlack;
use slakio_slack::Client;
use slakio_slack::adapter::LOOKUP_BATCH;
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_world::World;
use std::collections::HashSet;
use std::time::Duration;

const TOKEN: &str = concat!("xo", "xc-TEST");

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

async fn next(handle: &mut BackendHandle) -> Envelope {
    tokio::time::timeout(Duration::from_secs(10), handle.rx.recv()).await.expect("an answer").expect("open")
}

fn ask(handle: &BackendHandle, id: u64, command: Command) {
    handle.send(Request { id: Generation(id), workspace: None, command }).unwrap();
}

/// The ids asked for in each edge lookup, in order.
fn lookups(fake: &FakeSlack) -> Vec<Vec<String>> {
    fake.log()
        .iter()
        .filter(|l| l.method() == "users/info")
        .map(|l| {
            let body: serde_json::Value = serde_json::from_str(&l.body).unwrap();
            body["updated_ids"].as_object().unwrap().keys().cloned().collect()
        })
        .collect()
}

#[test]
fn dms_and_authors_show_names_looked_up_in_batches_once() {
    rt().block_on(async {
        let fake = FakeSlack::start(World::new(7)).await.unwrap();
        let world = World::new(7);
        let s = world.snapshot();
        let ws = s.workspaces[0].clone();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client = Client::new(Credentials::new(TOKEN, "c"), Endpoints::local(fake.addr()).unwrap(), governor);
        let mut handle = slakio_slack::serve(client);
        ask(&handle, 1, Command::Boot);
        let Event::Booted(snapshot) = next(&mut handle).await.event else { panic!("booted") };

        // Every DM is named after its person, not their id.
        for c in snapshot.conversations.iter().filter(|c| matches!(c.kind, ConversationKind::Dm { .. })) {
            let ConversationKind::Dm { user } = &c.kind else { unreachable!() };
            let person = s.users.iter().find(|u| &u.id == user).unwrap();
            assert_eq!(c.name.unsanitized(), person.display_name.unsanitized(), "{}", user.as_str());
        }
        let boot_lookups = lookups(&fake);
        assert!(!boot_lookups.is_empty());
        assert!(boot_lookups.iter().all(|b| b.len() <= LOOKUP_BATCH), "batches of at most {LOOKUP_BATCH}");

        // A page's authors come with their names; nobody is asked for twice.
        let channel = s.conversations.iter().find(|c| c.workspace == ws.id && world.message_count(&c.id) > 30).unwrap();
        let target = Target::Conversation { workspace: ws.id.clone(), conversation: channel.id.clone() };
        ask(&handle, 2, Command::History { target: target.clone(), before: None, limit: 28 });
        let Event::History(page) = next(&mut handle).await.event else { panic!("a page") };
        for m in page.messages.iter().filter(|m| m.user != ws.me) {
            let named =
                page.users.iter().find(|u| u.id == m.user).unwrap_or_else(|| panic!("{} named", m.user.as_str()));
            let person = s.users.iter().find(|u| u.id == m.user).unwrap();
            assert_eq!(named.display_name.unsanitized(), person.display_name.unsanitized());
        }
        let all = lookups(&fake);
        let mut seen = HashSet::new();
        for id in all.iter().flatten() {
            assert!(seen.insert(id.clone()), "{id} asked for twice");
        }
        assert!(all.len() <= boot_lookups.len() + 1, "a page's authors in one lookup at most: {all:?}");

        // The same page again: everyone is known, nothing more is asked.
        let before = all.len();
        ask(&handle, 3, Command::History { target, before: None, limit: 28 });
        let _ = next(&mut handle).await;
        assert_eq!(lookups(&fake).len(), before);
        assert!(fake.methods().iter().all(|m| m != "users.list"), "never a directory");
    });
}
