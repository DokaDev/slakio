//! Starred as Slack's desktop app places it, from a start bundle read against the fake Slack: a
//! starred DM with an app is never in Starred (with no Apps section listed it goes to Direct
//! messages), and a starred DM Slack says is closed stays placed in Starred, marked closed (the
//! UI leaves it out of the list).

use serde_json::json;
use slakio_core::backend::{Command, Envelope, Event, Generation, Request, Snapshot};
use slakio_core::model::{ConversationKind, SectionKind};
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault};
use slakio_slack::Client;
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_world::World;
use std::time::Duration;

/// The start bundle with three starred DMs (open with a person, closed with a person, open with
/// an app) and one starred channel, read by the backend from the fake; that snapshot.
fn booted() -> Snapshot {
    let world = World::demo();
    let s = world.snapshot();
    let ws = &s.workspaces[0];
    let of_ws = |bot: bool| s.users.iter().filter(move |u| u.workspace == ws.id && u.id != ws.me && u.bot == bot);
    let mut people = of_ws(false);
    let (ann, bob) = (people.next().unwrap(), people.next().unwrap());
    let app = of_ws(true).next().unwrap();
    // A channel of the world's Favorites (none of the user's own sections lists it).
    let favorites = s.sections.iter().find(|x| x.workspace == ws.id && x.kind == SectionKind::Favorites).unwrap();
    let channel = s.conversations.iter().find(|c| c.section == favorites.id).unwrap();
    let boot = json!({
        "ok": true,
        "self": {"id": ws.me.as_str(), "name": "me"},
        "team": {"id": ws.id.as_str(), "name": "Team"},
        "starred": [channel.id.as_str(), "DSTAROPEN", "DSTARSHUT", "DSTARAPP"],
        "is_open": ["DSTAROPEN", "DSTARAPP"],
        "channels": [{"id": channel.id.as_str(), "name": "starred-room", "is_channel": true}],
        "ims": [
            {"id": "DSTAROPEN", "user": ann.id.as_str(), "is_im": true, "is_open": true},
            {"id": "DSTARSHUT", "user": bob.id.as_str(), "is_im": true, "is_open": false},
            {"id": "DSTARAPP", "user": app.id.as_str(), "is_im": true, "is_open": true},
        ],
    });
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let fake = FakeSlack::start(World::demo()).await.unwrap();
        fake.fail_next("client.userBoot", Fault::Answer(boot));
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let creds = Credentials::new(concat!("xo", "xc-TEST"), "cookie-TEST");
        let client = Client::new(creds, Endpoints::local(fake.addr()).unwrap(), governor);
        let mut handle = slakio_slack::serve(client);
        handle.send(Request { id: Generation(1), workspace: None, command: Command::Boot }).unwrap();
        let e: Envelope =
            tokio::time::timeout(Duration::from_secs(20), handle.rx.recv()).await.expect("an answer").unwrap();
        match e.event {
            Event::Booted(s) => *s,
            other => panic!("{other:?}"),
        }
    })
}

#[test]
fn a_starred_app_dm_is_not_placed_in_starred_and_a_closed_one_is_marked() {
    let s = booted();
    let kind_of = |id: &str| {
        let c = s.conversations.iter().find(|c| c.id.as_str() == id).unwrap_or_else(|| panic!("{id} listed"));
        let section = s.sections.iter().find(|x| x.id == c.section).unwrap();
        (section.kind, c.closed)
    };
    let app = s.conversations.iter().find(|c| c.id.as_str() == "DSTARAPP").unwrap();
    assert!(matches!(app.kind, ConversationKind::Dm { .. }));
    assert_eq!(kind_of("DSTAROPEN"), (SectionKind::Favorites, false), "an open starred DM is in Starred");
    assert_eq!(kind_of("DSTARSHUT"), (SectionKind::Favorites, true), "a closed starred DM: Starred, closed");
    // The demo workspace lists no Apps section: the app's DM is in Direct messages.
    assert!(!s.sections.iter().any(|x| x.kind == SectionKind::Apps), "{:?}", s.sections);
    assert_eq!(kind_of("DSTARAPP"), (SectionKind::DirectMessages, false), "a starred app DM is not in Starred");
}
