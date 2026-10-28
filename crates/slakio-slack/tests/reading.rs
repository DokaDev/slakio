//! Reading a workspace for real, against the fake Slack: mute, do not disturb, Slack Connect and
//! unread counts at the start; older pages of a conversation and their edges (duplicates,
//! deleted messages, an empty page, the end); a thread read forward by cursor and served
//! backwards; Block Kit and bots. The edge answers are synthetic, written here.

use serde_json::json;
use slakio_core::backend::{BackendHandle, Command, Envelope, Event, Generation, Page, Request, Snapshot};
use slakio_core::model::{ConversationKind, Org, Presence, Target, Ts};
use slakio_core::secret::Credentials;
use slakio_fake::{FakeSlack, Fault};
use slakio_slack::Client;
use slakio_slack::governor::{FakeClock, Governor, Limits};
use slakio_slack::request::Endpoints;
use slakio_world::World;
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

    async fn boot(&mut self) -> Snapshot {
        match self.ask(Command::Boot).await {
            Event::Booted(s) => *s,
            other => panic!("{other:?}"),
        }
    }

    async fn page(&mut self, target: &Target, before: Option<Ts>) -> Page {
        match self.ask(Command::History { target: target.clone(), before, limit: 28 }).await {
            Event::History(p) => *p,
            other => panic!("{other:?}"),
        }
    }

    fn calls(&self, method: &str) -> usize {
        self.fake.methods().iter().filter(|m| *m == method).count()
    }

    fn target(&self, name: &str) -> Target {
        let s = self.world.snapshot();
        let c = s.conversations.iter().find(|c| c.workspace == s.workspaces[0].id && c.name == name).unwrap();
        Target::Conversation { workspace: c.workspace.clone(), conversation: c.id.clone() }
    }
}

fn ts(m: &[slakio_core::model::Message]) -> Vec<Ts> {
    m.iter().map(|m| m.ts).collect()
}

#[test]
fn the_start_reads_mute_do_not_disturb_slack_connect_and_counts() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let s = r.boot().await;
        let world = r.world.snapshot();
        let ws = &s.workspaces[0].id;
        let listed = world
            .conversations
            .iter()
            .filter(|c| &c.workspace == ws && !matches!(c.kind, ConversationKind::GroupDm { .. }));
        let mut checked = (0, 0, 0);
        for want in listed {
            let got = s.conversations.iter().find(|c| c.id == want.id).unwrap();
            assert_eq!(got.muted, want.muted, "{}", got.id);
            if !got.is_dm() {
                assert_eq!(got.external, want.external, "{}", got.id);
            }
            assert_eq!(got.unread > 0, want.unread > 0, "{}", got.id);
            if !got.is_dm() {
                assert_eq!(got.mentions, want.mentions, "{}", got.id);
            }
            checked.0 += usize::from(got.muted);
            checked.1 += usize::from(got.external);
            checked.2 += usize::from(got.mentions > 0);
        }
        assert!(
            checked.0 > 0 && checked.1 > 0 && checked.2 > 0,
            "the world has muted, shared and mentioning conversations: {checked:?}"
        );
        assert!(!s.counts_failed);
        // The DMs' people: in do not disturb as the world says, a peer of another organization.
        for u in &s.users {
            let want = world.users.iter().find(|w| w.id == u.id && &w.workspace == ws).unwrap();
            if u.id != s.workspaces[0].me {
                assert_eq!(u.presence == Presence::Dnd, want.presence == Presence::Dnd, "{}", u.id);
            }
            assert_eq!(matches!(u.org, Org::External(_)), matches!(want.org, Org::External(_)), "{}", u.id);
        }
        assert!(s.users.iter().any(|u| u.presence == Presence::Dnd));
        // The shared channel's partners, met as its messages' authors, are of another organization.
        let shared = r.target(slakio_world::names::SHARED);
        let p = r.page(&shared, None).await;
        assert!(p.users.iter().any(|u| matches!(u.org, Org::External(_))), "{:?}", p.users);
        let dnd = r.fake.log().into_iter().find(|l| l.method() == "dnd.teamInfo").unwrap();
        assert!(dnd.fields().iter().any(|(k, _)| *k == "users"), "asked about the DMs' people");
    });
}

#[test]
fn counts_or_do_not_disturb_that_fail_are_unknown_not_zero() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.fake.fail_next("client.counts", Fault::Api("internal_error".into()));
        r.fake.fail_next("dnd.teamInfo", Fault::Status { status: 500, retry_after: None });
        let s = r.boot().await;
        assert!(s.counts_failed, "the counts are unknown");
        let me = &s.workspaces[0].me;
        assert!(
            s.users.iter().filter(|u| &u.id != me).all(|u| u.presence == Presence::Unknown),
            "not 'available' either"
        );
    });
}

#[test]
fn older_pages_follow_one_another_to_the_start() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.boot().await;
        let target = r.target("deploys");
        let id = target.conversation().clone();
        let total = r.world.message_count(&id);
        let mut seen = r.page(&target, None).await.messages;
        let mut complete = false;
        let mut pages = 0;
        while !complete {
            let before = seen.first().map(|m| m.ts);
            let p = r.page(&target, before).await;
            assert!(p.messages.iter().all(|m| Some(m.ts) < before), "older only");
            complete = p.complete;
            seen.splice(0..0, p.messages);
            pages += 1;
            assert!(pages < 100);
        }
        assert_eq!(seen.len(), total);
        let (all, _) = r.world.history(&id, None, total);
        assert_eq!(ts(&seen), ts(&all), "no gap, no message twice");
        assert_eq!(r.calls("conversations.history"), pages);
        let older = r.fake.log().into_iter().rfind(|l| l.method() == "conversations.history").unwrap();
        assert!(older.fields().iter().any(|(k, _)| *k == "latest"));
    });
}

#[test]
fn an_older_page_s_duplicates_deleted_messages_and_emptiness_are_handled() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.boot().await;
        let target = r.target("deploys");
        let m = |ts: &str, extra: serde_json::Value| {
            let mut v = json!({"type": "message", "ts": ts, "user": "UDEMOA001", "text": format!("at {ts}")});
            v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            v
        };
        // Newest first as Slack lists them; one listed twice, one deleted, one hidden, one at
        // the time asked before (not older: dropped).
        let answer = json!({"ok": true, "has_more": true, "messages": [
            m("1700000300.000000", json!({})),
            m("1700000200.000000", json!({})),
            m("1700000200.000000", json!({})),
            m("1700000150.000000", json!({"subtype": "message_deleted"})),
            m("1700000120.000000", json!({"hidden": true})),
            m("1700000100.000000", json!({"subtype": "tombstone", "text": "This message was deleted.", "reply_count": 2, "latest_reply": "1700000400.000000"})),
        ]});
        r.fake.fail_next("conversations.history", Fault::Answer(answer));
        let p = r.page(&target, Some(Ts(1_700_000_300_000_000))).await;
        assert_eq!(ts(&p.messages), [Ts(1_700_000_100_000_000), Ts(1_700_000_200_000_000)]);
        assert!(p.messages[0].thread.is_some(), "a deleted thread's own message keeps its replies");
        assert!(!p.complete, "Slack says there is more");
        // Nothing older, though Slack does not say it ended: the end all the same (asking again
        // would ask for the same).
        r.fake.fail_next("conversations.history", Fault::Answer(json!({"ok": true, "has_more": true, "messages": []})));
        assert!(r.page(&target, Some(Ts(1_700_000_100_000_000))).await.complete);
        // No word on more: unknown is not the end.
        r.fake.fail_next(
            "conversations.history",
            Fault::Answer(json!({"ok": true, "messages": [m("1700000050.000000", json!({}))]})),
        );
        assert!(!r.page(&target, Some(Ts(1_700_000_100_000_000))).await.complete);
    });
}

#[test]
fn a_thread_is_read_forward_once_and_served_backwards() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.boot().await;
        let parent = r.target("long-threads");
        let conv = parent.conversation().clone();
        let first = r
            .world
            .history(&conv, None, r.world.message_count(&conv))
            .0
            .into_iter()
            .find(|m| m.thread.is_some_and(|t| t.replies > 1000))
            .unwrap();
        let thread =
            Target::Thread { workspace: parent.workspace().clone(), conversation: conv.clone(), thread: first.ts };
        let (whole, _) = r.world.thread(&conv, first.ts, None, usize::MAX).unwrap();
        let newest = r.page(&thread, None).await;
        assert_eq!(ts(&newest.messages), ts(&whole[whole.len() - 28..]), "the newest page is the thread's end");
        assert!(!newest.complete);
        let calls = r.calls("conversations.replies");
        assert_eq!(calls, whole.len().div_ceil(200), "read forward, 200 a request");
        let cursors = r
            .fake
            .log()
            .into_iter()
            .filter(|l| l.method() == "conversations.replies")
            .filter(|l| l.fields().iter().any(|(k, _)| *k == "cursor"))
            .count();
        assert_eq!(cursors, calls - 1, "each after the first by the cursor Slack gave");
        // Older pages, back to the thread's own message, without asking Slack again.
        let mut seen = newest.messages;
        loop {
            let p = r.page(&thread, seen.first().map(|m| m.ts)).await;
            seen.splice(0..0, p.messages);
            if p.complete {
                break;
            }
        }
        assert_eq!(ts(&seen), ts(&whole));
        assert_eq!(seen[0].ts, first.ts, "the thread starts with its own message");
        assert_eq!(r.calls("conversations.replies"), calls, "served from what was read");
        // Opening it again reads it again (new replies).
        r.page(&thread, None).await;
        assert_eq!(r.calls("conversations.replies"), calls * 2);
    });
}

#[test]
fn a_thread_whose_cursor_does_not_move_ends() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.boot().await;
        let parent = r.target("deploys");
        let thread = Target::Thread { workspace: parent.workspace().clone(), conversation: parent.conversation().clone(), thread: Ts(1_700_000_000_000_000) };
        let stuck = |ts: &str| json!({"ok": true, "has_more": true, "messages": [{"type": "message", "ts": ts, "user": "U1", "text": "r"}], "response_metadata": {"next_cursor": "same"}});
        r.fake.fail_next("conversations.replies", Fault::Answer(stuck("1700000000.000000")));
        r.fake.fail_next("conversations.replies", Fault::Answer(stuck("1700000001.000000")));
        r.fake.fail_next("conversations.replies", Fault::Answer(stuck("1700000002.000000")));
        let p = r.page(&thread, None).await;
        assert_eq!(r.calls("conversations.replies"), 2, "the same cursor again: the end");
        assert_eq!(p.messages.len(), 2);
    });
}

#[test]
fn block_kit_and_bots_are_read_as_text_and_names() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.boot().await;
        let target = r.target("alerts");
        let answer = json!({"ok": true, "history": {"has_more": false, "messages": [
            {"type": "message", "subtype": "bot_message", "ts": "1700000000.000100", "bot_id": "BALERT", "username": "Grafana",
             "text": "fallback only",
             "blocks": [{"type": "section", "text": {"type": "mrkdwn", "text": "*CPU* high on <https://grafana.example|web-1>"}}]},
            {"type": "message", "ts": "1700000000.000200", "user": "UDEMOA001", "text": "",
             "files": [{"name": "report.pdf"}]},
        ]}, "users": []});
        r.fake.fail_next("conversations.view", Fault::Answer(answer));
        let p = r.page(&target, None).await;
        assert_eq!(p.messages[0].text.unsanitized(), "*CPU* high on <https://grafana.example|web-1>");
        assert_eq!(p.messages[0].user.as_str(), "BALERT");
        assert_eq!(p.messages[1].text.unsanitized(), "📎 report.pdf");
        let bot = p.users.iter().find(|u| u.id.as_str() == "BALERT").expect("the bot, by the name it posts as");
        assert!(bot.bot && bot.display_name.unsanitized() == "Grafana");
        assert!(p.complete);
        assert!(!r.fake.log().into_iter().any(|l| l.method() == "users/info" && l.body.contains("BALERT")), "a bot is not looked up");
    });
}
