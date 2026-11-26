//! Starred as Slack's desktop app shows it, from a start bundle read for real by the Slack
//! backend against the fake Slack: a starred DM that Slack says is closed is not listed, and a
//! starred DM with an app never is (with no Apps section listed it goes to Direct messages); a
//! starred open DM with a person and a starred channel stay in Starred.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "this file drives the boot alone, with no keys or failures")]
mod scripted;

use scripted::Scripted;
use serde_json::json;
use slakio_core::backend::{Command, Envelope, Event, Generation, Request, Snapshot};
use slakio_core::model::{ConversationKind, SectionKind};
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault};
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_tui::app::model::Row;
use slakio_tui::app::shell::View;
use slakio_world::World;
use std::time::Duration;

/// The start bundle with three starred DMs (open with a person, closed with a person, open with
/// an app) and one starred channel, read by the backend from the fake; that snapshot.
fn booted_from_slack() -> Snapshot {
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
        let client = slakio_slack::Client::new(creds, Endpoints::local(fake.addr()).unwrap(), governor);
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

/// The ids Home lists under each section kind, in order.
fn listed(s: &Scripted) -> Vec<(SectionKind, String)> {
    let (mut kind, mut out) = (None, Vec::new());
    for r in s.d.app.list_rows() {
        match r {
            Row::Section(i) => kind = Some(s.d.app.model.section(i).kind),
            Row::Conversation(i) => {
                out.push((kind.unwrap(), s.d.app.model.conversation(i).id.as_str().to_string()));
            }
            _ => {}
        }
    }
    out
}

#[test]
fn starred_lists_no_closed_dm_and_no_app_dm() {
    let snapshot = booted_from_slack();
    let app = snapshot.conversations.iter().find(|c| c.id.as_str() == "DSTARAPP").unwrap();
    assert!(matches!(app.kind, ConversationKind::Dm { .. }));
    let mut s = Scripted::booted(true, Some(snapshot));
    s.d.app.show_unfolded(View::Home);
    let rows = listed(&s);
    let starred: Vec<&str> =
        rows.iter().filter(|(k, _)| *k == SectionKind::Favorites).map(|(_, id)| id.as_str()).collect();
    assert!(starred.contains(&"DSTAROPEN"), "an open starred DM stays in Starred: {rows:?}");
    assert_eq!(starred.len(), 2, "the starred channel and the open DM only: {rows:?}");
    assert!(!rows.iter().any(|(_, id)| id == "DSTARSHUT"), "a closed starred DM is not listed: {rows:?}");
    // The demo workspace lists no Apps section: the app's DM is in Direct messages.
    assert!(rows.contains(&(SectionKind::DirectMessages, "DSTARAPP".to_string())), "{rows:?}");
    // The closed one is still known: the DMs view lists it.
    s.d.app.show_unfolded(View::Dms);
    assert!(listed_any(&s, "DSTARSHUT"), "the DMs view lists every DM");
}

fn listed_any(s: &Scripted, id: &str) -> bool {
    (s.d.app.list_rows().iter())
        .any(|r| matches!(r, Row::Conversation(i) if s.d.app.model.conversation(*i).id.as_str() == id))
}

/// The ids `d`'s list shows under each section kind, in order.
fn demo_listed(d: &demo::Demo) -> Vec<(SectionKind, String)> {
    let (mut kind, mut out) = (None, Vec::new());
    for r in d.app.list_rows() {
        match r {
            Row::Section(i) => kind = Some(d.app.model.section(i).kind),
            Row::Conversation(i) => out.push((kind.unwrap(), d.app.model.conversation(i).id.as_str().to_string())),
            _ => {}
        }
    }
    out
}

/// The demo's `B side` lists Starred first with its open starred DM (not the closed one) and
/// Apps last with the DM with an app; the DMs view still lists the closed one.
#[test]
fn the_demo_shows_an_app_dm_in_apps_and_hides_a_closed_starred_dm() {
    let mut d = demo::Demo::new(120, 40);
    d.keys("space W j enter");
    assert_eq!(d.app.workspace(), 1);
    let rows = demo_listed(&d);
    let ids = |kind: SectionKind| -> Vec<&str> {
        rows.iter().filter(|(k, _)| *k == kind).map(|(_, id)| id.as_str()).collect()
    };
    assert_eq!(ids(SectionKind::Favorites), ["DDEMOBS001"], "the open starred DM only: {rows:?}");
    assert_eq!(rows.first().map(|(k, _)| *k), Some(SectionKind::Favorites), "Starred first: {rows:?}");
    assert_eq!(ids(SectionKind::Apps), ["DDEMOBP001"], "{rows:?}");
    assert_eq!(rows.last().map(|(k, _)| *k), Some(SectionKind::Apps), "Apps last: {rows:?}");
    assert!(!rows.iter().any(|(_, id)| id == "DDEMOBS002"), "the closed starred DM is not listed: {rows:?}");
    let s = d.screen();
    assert!(s.contains("Starred") && s.contains("Hana"), "{s}");
    d.keys("G");
    let s = d.screen();
    assert!(s.contains("Apps") && s.contains("Grafana"), "{s}");
    d.keys("space d");
    let dms: Vec<String> = demo_listed_dms(&d);
    assert!(dms.iter().any(|id| id == "DDEMOBS002"), "the DMs view lists the closed one: {dms:?}");
}

fn demo_listed_dms(d: &demo::Demo) -> Vec<String> {
    (d.app.list_rows().into_iter())
        .filter_map(|r| match r {
            Row::Conversation(i) => Some(d.app.model.conversation(i).id.as_str().to_string()),
            _ => None,
        })
        .collect()
}
