//! Another workspace's emoji in a shared channel, against the fake Slack: a reaction or an emoji
//! of rich text that comes with the address of its picture gives the message that address, for
//! a name a shortcode may have (its Hangul composed) on the emoji host only, once a name, at
//! most [`EMOJI_PICTURES_MAX`]; an address anywhere else is dropped. The answers are synthetic.

use serde_json::{Value, json};
use slakio_core::backend::{BackendHandle, Command, Envelope, Event, Generation, Page, Request};
use slakio_core::images::EMOJI_HOSTS;
use slakio_core::model::{EMOJI_PICTURES_MAX, Target, Ts};
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault};
use slakio_slack::Client;
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_world::World;
use std::time::Duration;

const TOKEN: &str = concat!("xo", "xc-TEST");

/// A synthetic Korean name ("gamsahaeyong"), typed and as a Mac writes it.
const THANKS_NFC: &str = "\u{AC10}\u{C0AC}\u{D574}\u{C6A9}";
const THANKS_NFD: &str = "\u{1100}\u{1161}\u{11B7}\u{1109}\u{1161}\u{1112}\u{1162}\u{110B}\u{116D}\u{11BC}";

fn partner(name: &str) -> String {
    format!("https://{}/TPARTNER/{name}/abc.png", EMOJI_HOSTS[0])
}

async fn page(answer: Value) -> Page {
    let world = World::demo();
    let s = world.snapshot();
    let c = s.conversations.iter().find(|c| c.workspace == s.workspaces[0].id && c.name == "general").unwrap();
    let target = Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() };
    let fake = FakeSlack::start(World::demo()).await.unwrap();
    let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
    let client = Client::new(Credentials::new(TOKEN, "cookie-TEST"), Endpoints::local(fake.addr()).unwrap(), governor);
    let handle: BackendHandle = slakio_slack::serve(client);
    fake.fail_next("conversations.history", Fault::Answer(answer));
    // An older page: read with `conversations.history`.
    let command = Command::History { target, before: Some(Ts(1_800_000_000_000_000)), limit: 28 };
    let mut handle = handle;
    handle.send(Request { id: Generation(1), workspace: None, command }).unwrap();
    let e: Envelope = tokio::time::timeout(Duration::from_secs(20), handle.rx.recv()).await.unwrap().unwrap();
    match e.event {
        Event::History(p) => *p,
        other => panic!("{other:?}"),
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

/// A message of the partner's person with `reactions` and the rich-text `elements` of its text.
fn shared(ts: &str, reactions: Value, elements: Value) -> Value {
    json!({"type": "message", "ts": ts, "user": "UPARTNER1", "text": "hi", "reactions": reactions,
        "blocks": [{"type": "rich_text", "elements": [{"type": "rich_text_section", "elements": elements}]}]})
}

fn given(p: &Page, i: usize) -> Vec<(String, String)> {
    p.messages[i].emoji_pictures.iter().map(|(n, a)| (n.unsanitized().to_string(), a.clone())).collect()
}

#[test]
fn another_workspace_s_emoji_addresses_are_read_from_its_reactions_and_text() {
    rt().block_on(async {
        let answer = json!({"ok": true, "messages": [shared(
            "1700000100.000000",
            json!([{"name": THANKS_NFD, "count": 4, "users": ["UPARTNER1"], "url": partner("thanks")},
                   {"name": "plain", "count": 1, "users": ["UPARTNER1"]}]),
            json!([{"type": "text", "text": "hi "}, {"type": "emoji", "name": "partner-wave", "url": partner("wave")},
                   {"type": "emoji", "name": "smile"}]),
        )]});
        let p = page(answer).await;
        assert_eq!(
            given(&p, 0),
            [(THANKS_NFC.to_string(), partner("thanks")), ("partner-wave".to_string(), partner("wave"))]
        );
    });
}

/// A guard: an address off the emoji host, a name no shortcode has, a name given twice and more
/// than [`EMOJI_PICTURES_MAX`] are left out; a message without any gives none.
#[test]
fn an_emoji_address_off_the_emoji_host_is_dropped() {
    rt().block_on(async {
        let many: Vec<Value> = (0..EMOJI_PICTURES_MAX + 4)
            .map(|i| json!({"type": "emoji", "name": format!("e{i}"), "url": partner(&format!("e{i}"))}))
            .collect();
        let answer = json!({"ok": true, "messages": [
            shared("1700000300.000000", json!([]), Value::Array(many)),
            shared(
                "1700000200.000000",
                json!([{"name": "evil", "count": 1, "users": [], "url": "https://evil.example/x.png"},
                       {"name": "bad name", "count": 1, "users": [], "url": partner("bad")},
                       {"name": "twice", "count": 1, "users": [], "url": partner("twice")}]),
                json!([{"type": "emoji", "name": "twice", "url": partner("other")},
                       {"type": "emoji", "name": "porty", "url": format!("https://{}:8443/T/p/a.png", EMOJI_HOSTS[0])},
                       {"type": "emoji", "name": "plain", "url": format!("http://{}/T/p/a.png", EMOJI_HOSTS[0])}]),
            ),
            json!({"type": "message", "ts": "1700000100.000000", "user": "UDEMOA001", "text": "plain"}),
        ]});
        let p = page(answer).await;
        assert_eq!(given(&p, 0), [], "nothing given");
        assert_eq!(given(&p, 1), [("twice".to_string(), partner("twice"))]);
        assert_eq!(given(&p, 2).len(), EMOJI_PICTURES_MAX);
    });
}
