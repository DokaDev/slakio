//! Searching a workspace's messages against the fake Slack: one `search.messages` request a
//! page (the query as typed, newest first, the words matched marked), the authors named as a
//! page's are; a reply told apart by its link; many pages, none found, hostile answers, Slack
//! asking to slow down, a newer search superseding one on its way. Nothing else is sent.

use slakio_core::backend::{
    BackendHandle, Command, Envelope, Event, FailKind, Found, Generation, MATCH_START, Request, SEARCH_PAGE,
};
use slakio_core::model::Ts;
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
    /// Signed in and booted; the log starts after the boot.
    async fn new() -> Self {
        let fake = FakeSlack::start(World::demo()).await.unwrap();
        let governor = Governor::new(Limits::default(), Box::new(FakeClock::default()), true);
        let client =
            Client::new(Credentials::new(TOKEN, "cookie-TEST"), Endpoints::local(fake.addr()).unwrap(), governor);
        let mut r = Self { fake, handle: slakio_slack::serve(client), world: World::demo(), id: 0 };
        assert!(matches!(r.ask(Command::Boot).await, Event::Booted(_)));
        r
    }

    async fn ask(&mut self, command: Command) -> Event {
        self.id += 1;
        self.handle.send(Request { id: Generation(self.id), workspace: None, command }).unwrap();
        let e = next(&mut self.handle).await;
        assert_eq!(e.generation, Generation(self.id));
        e.event
    }

    async fn search(&mut self, query: &str, page: u32) -> Found {
        match self.ask(Command::Search { query: query.into(), page }).await {
            Event::Found(f) => *f,
            other => panic!("{other:?}"),
        }
    }

    /// The `search.messages` requests since the boot, their fields decoded.
    fn searches(&self) -> Vec<Vec<(String, String)>> {
        (self.fake.log().iter())
            .filter(|l| l.method() == "search.messages")
            .map(|l| l.fields().into_iter().map(|(k, v)| (k.to_string(), decode(v))).collect())
            .collect()
    }

    /// The methods asked for after the boot.
    fn after_boot(&self) -> Vec<String> {
        let m = self.fake.methods();
        let boot = m.iter().rposition(|m| m == "dnd.teamInfo").map_or(0, |i| i + 1);
        m[boot..].to_vec()
    }
}

async fn next(h: &mut BackendHandle) -> Envelope {
    tokio::time::timeout(Duration::from_secs(20), h.rx.recv()).await.expect("an answer").unwrap()
}

fn decode(v: &str) -> String {
    let mut out = Vec::new();
    let b = v.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                out.push(u8::from_str_radix(&v[i + 1..i + 3], 16).unwrap());
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8(out).unwrap()
}

fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

#[test]
fn a_query_goes_as_typed_and_its_page_comes_newest_first_with_the_words_marked() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let query = format!("\"deploy\" in:#{}", names::LONG_THREADS);
        let found = r.search(&query, 1).await;
        let ws = r.world.snapshot().workspaces[0].id.clone();
        let all = r.world.search(&ws, &query);
        assert!(!all.is_empty());
        assert_eq!(found.total as usize, all.len());
        assert_eq!(found.pages as usize, all.len().div_ceil(SEARCH_PAGE as usize));
        assert_eq!(found.page, 1);
        let want: Vec<Ts> = all.iter().take(SEARCH_PAGE as usize).map(|h| h.message.ts).collect();
        assert_eq!(found.hits.iter().map(|h| h.ts).collect::<Vec<_>>(), want);
        assert!(found.hits.iter().all(|h| h.text.unsanitized().contains(MATCH_START)));
        // A reply is told by its link; a message is not one.
        for (h, w) in found.hits.iter().zip(&all) {
            assert_eq!(h.thread, w.thread, "{h:?}");
        }
        let asked = r.searches();
        assert_eq!(asked.len(), 1);
        assert_eq!(field(&asked[0], "query"), Some(query.as_str()), "verbatim");
        assert_eq!(field(&asked[0], "count"), Some("20"));
        assert_eq!(field(&asked[0], "page"), Some("1"));
        assert_eq!(field(&asked[0], "highlight"), Some("true"));
        assert_eq!(field(&asked[0], "sort"), Some("timestamp"));
        assert_eq!(field(&asked[0], "sort_dir"), Some("desc"));
        // The authors by name, as a page's are: nothing else is asked.
        assert!(r.after_boot().iter().all(|m| m == "search.messages" || m == "users/info"), "{:?}", r.after_boot());
        assert!(found.users.iter().all(|u| found.hits.iter().any(|h| h.user == u.id)));
    });
}

#[test]
fn many_pages_come_one_request_each_and_none_found_is_empty() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let first = r.search("the", 1).await;
        assert!(first.pages > 3, "{}", first.pages);
        let third = r.search("the", 3).await;
        assert_eq!(third.page, 3);
        assert!(third.hits.first().unwrap().ts < first.hits.last().unwrap().ts, "older on later pages");
        assert_eq!(r.searches().len(), 2);
        let none = r.search("zzqqxx-nothing", 1).await;
        assert!(none.hits.is_empty());
        assert_eq!((none.total, none.pages), (0, 0));
        // An empty query sends nothing.
        let before = r.fake.log().len();
        let empty = r.search("   ", 1).await;
        assert!(empty.hits.is_empty());
        assert_eq!(r.fake.log().len(), before);
        // A long query is cut, the page is kept in Slack's range.
        r.search(&"x".repeat(2000), 500).await;
        let last = r.searches().pop().unwrap();
        assert_eq!(field(&last, "query").unwrap().chars().count(), slakio_slack::search::QUERY_MAX);
        assert_eq!(field(&last, "page"), Some("100"));
    });
}

#[test]
fn a_hostile_answer_is_read_tolerantly_and_kept_as_remote_text() {
    rt().block_on(async {
        let mut r = Run::new().await;
        let evil = "\u{1b}]8;;https://evil.test\u{7}\u{1b}[2Jclick\u{202e}\u{e000}x\u{e001}";
        let answer = serde_json::json!({"ok": true, "messages": {"total": 4, "paging": {"page": 1, "pages": 1}, "matches": [
            {"ts": "1767603600.000100", "channel": {"id": "C1", "name": evil}, "user": "U1", "username": evil,
             "text": evil, "permalink": "https://evil.test/archives/C1/p1?thread_ts=1767600000.000200&cid=C9"},
            {"ts": "not-a-time", "channel": {"id": "C1"}, "text": "x"},
            {"ts": "1767603600.000200", "text": "no channel"},
            {"ts": "1767603600.000300", "channel": {"id": "C1"}, "text": 7, "permalink": ["x"]}
        ]}});
        r.fake.fail_next("search.messages", Fault::Answer(answer));
        let found = r.search("click", 1).await;
        assert_eq!(found.hits.len(), 1, "{:?}", found.hits);
        let h = &found.hits[0];
        assert_eq!(h.text.unsanitized(), evil, "the UI sanitises it");
        assert_eq!(h.name.unsanitized(), evil);
        assert_eq!(h.thread, None, "a link naming another conversation names no thread");
        // Paging missing or broken: one page of what came.
        r.fake.fail_next("search.messages", Fault::Answer(serde_json::json!({"ok": true, "messages": {"matches": 5}})));
        let odd = r.search("click", 1).await;
        assert!(odd.hits.is_empty() && odd.pages == 0);
        r.fake.fail_next("search.messages", Fault::Malformed);
        assert!(matches!(r.ask(Command::Search { query: "x".into(), page: 1 }).await, Event::Failed { .. }));
    });
}

#[test]
fn slack_refusing_the_search_fails_it_as_its_own_error() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.fake.fail_next("search.messages", Fault::Api("not_allowed_token_type".into()));
        let refused = r.ask(Command::Search { query: "the".into(), page: 1 }).await;
        assert!(matches!(refused, Event::Failed { kind: FailKind::Api { .. }, .. }), "{refused:?}");
    });
}

#[test]
fn slack_asking_to_slow_down_fails_the_search_as_rate_limited() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.fake.fail_next("search.messages", Fault::Status { status: 429, retry_after: Some("7".into()) });
        let failed = r.ask(Command::Search { query: "the".into(), page: 1 }).await;
        let Event::Failed { kind, .. } = failed else { panic!("{failed:?}") };
        assert!(matches!(kind, FailKind::RateLimited { .. }), "{kind:?}");
        // Slack's pause is kept: the next search is not sent meanwhile.
        let paused = r.ask(Command::Search { query: "the".into(), page: 1 }).await;
        assert!(matches!(paused, Event::Failed { kind: FailKind::RateLimited { .. }, .. }), "{paused:?}");
        assert_eq!(r.searches().len(), 1, "nothing sent during the pause");
    });
}

#[test]
fn a_newer_search_supersedes_one_on_its_way_and_each_is_answered_once() {
    rt().block_on(async {
        let mut r = Run::new().await;
        r.fake.fail_next("search.messages", Fault::Hang);
        let ask = |id, query: &str| Request {
            id: Generation(id),
            workspace: None,
            command: Command::Search { query: query.into(), page: 1 },
        };
        r.handle.send(ask(100, "the")).unwrap();
        while r.searches().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        r.handle.send(ask(101, "deploy")).unwrap();
        let first = next(&mut r.handle).await;
        assert_eq!(first.generation, Generation(100));
        assert!(matches!(first.event, Event::Failed { kind: FailKind::Busy, .. }), "{first:?}");
        let second = next(&mut r.handle).await;
        assert_eq!(second.generation, Generation(101));
        assert!(matches!(second.event, Event::Found(_)), "{second:?}");
        // A page of a conversation is not a search: neither supersedes the other.
        let big = r.world.find(r.world.snapshot().workspaces[0].id.as_str(), names::BIG_HISTORY).unwrap().clone();
        let target = slakio_core::model::Target::Conversation { workspace: big.workspace, conversation: big.id };
        r.handle
            .send(Request {
                id: Generation(102),
                workspace: None,
                command: Command::History { target, before: None, limit: 28 },
            })
            .unwrap();
        r.handle.send(ask(103, "the")).unwrap();
        let mut got: Vec<(u64, bool)> = Vec::new();
        for _ in 0..2 {
            let e = next(&mut r.handle).await;
            got.push((e.generation.0, matches!(e.event, Event::Failed { .. })));
        }
        got.sort_unstable();
        assert_eq!(got, [(102, false), (103, false)]);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(r.handle.rx.try_recv().is_err(), "nothing answered twice");
        assert!(r.fake.hangs_closed() >= 1 || r.searches().len() == 3);
    });
}
