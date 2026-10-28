//! The cache in front of a backend, end to end over the protocol: a first run keeps what the
//! backend answers; the next run's boot and newest pages are answered from the cache before the
//! backend is even asked, then by the backend, whose answers are kept again. The backend is the
//! test (it holds the backend's end of the handle and answers by hand), the cache a real one in
//! a scratch directory.

use slakio_core::backend::{
    BackendEnd, BackendHandle, Capabilities, Command, Envelope, Event, FailKind, Generation, Page, Request,
};
use slakio_core::model::{Target, Ts, WorkspaceId};
use slakio_store::cache::{Cache, system_clock};
use slakio_store::{Caps, Store, layer};
use slakio_world::World;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const CAPS: Capabilities = Capabilities { demo: false, read_only: true };

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("slakio-layer-{}-{tag}", std::process::id())).join("cache");
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// One run: the UI's handle, the backend's end, the cache's faults.
struct Run {
    ui: BackendHandle,
    backend: BackendEnd,
    cache: Cache,
    faults: Arc<Mutex<Vec<String>>>,
}

/// Open the cache in `dir` (waiting for an earlier run's thread to let go of it, unless
/// `reader`) and put it in front of a backend of `workspace`.
async fn run(dir: &Path, workspace: &WorkspaceId, caps: Caps, reader: bool) -> Run {
    let store = open(dir, reader).await;
    let faults: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = Arc::clone(&faults);
    let cache =
        Cache::start(store, caps, system_clock(), Box::new(move |f| log.lock().unwrap().push(f.to_string()))).unwrap();
    let (inner, backend) = BackendHandle::pair(CAPS, 64);
    let ui = layer::serve(inner, cache.clone(), workspace.clone(), layer::Waits::default(), Box::new(|_| {}));
    Run { ui, backend, cache, faults }
}

async fn open(dir: &Path, reader: bool) -> Store {
    for _ in 0..100 {
        let store = Store::open(dir).unwrap().store;
        if store.read_only() == reader {
            return store;
        }
        drop(store);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the cache never opened as {}", if reader { "a reader" } else { "the writer" });
}

impl Run {
    fn ask(&self, id: u64, command: Command) {
        self.ui.send(Request { id: Generation(id), workspace: None, command }).unwrap();
    }

    async fn ui_next(&mut self) -> Envelope {
        tokio::time::timeout(Duration::from_secs(5), self.ui.rx.recv()).await.expect("an answer").expect("open")
    }

    async fn backend_next(&mut self) -> Request {
        tokio::time::timeout(Duration::from_secs(5), self.backend.rx.recv()).await.expect("a request").expect("open")
    }

    async fn answer(&self, id: Generation, event: Event) {
        self.backend.tx.send(Envelope { generation: id, event }).await.unwrap();
    }

    /// Every job asked of the cache so far done.
    async fn settled(&self) {
        let c = self.cache.clone();
        assert!(tokio::task::spawn_blocking(move || c.flush(Duration::from_secs(5))).await.unwrap());
    }

    fn no_faults(&self) {
        assert!(self.faults.lock().unwrap().is_empty(), "{:?}", self.faults.lock().unwrap());
    }
}

fn world() -> World {
    World::new(13)
}

/// The world's first workspace alone, as the Slack backend boots it.
fn snapshot(world: &World) -> slakio_core::backend::Snapshot {
    let mut s = world.snapshot().clone();
    let ws = s.workspaces[0].id.clone();
    s.workspaces.truncate(1);
    s.users.retain(|u| u.workspace == ws);
    s.sections.retain(|x| x.workspace == ws);
    s.conversations.retain(|c| c.workspace == ws);
    s.saved = None;
    s
}

fn target(world: &World, name: &str) -> Target {
    let ws = &world.snapshot().workspaces[0].id;
    Target::Conversation { workspace: ws.clone(), conversation: world.find(ws.as_str(), name).unwrap().id.clone() }
}

fn page(world: &World, t: &Target, before: Option<Ts>) -> Page {
    let (messages, complete) = world.history(t.conversation(), before, 28);
    Page { target: t.clone(), messages, complete, users: Vec::new(), cut: false }
}

fn history(t: &Target, before: Option<Ts>) -> Command {
    Command::History { target: t.clone(), before, limit: 28 }
}

/// A first run that boots and opens `general`: the cache keeps both.
async fn first_run(dir: &Path, w: &World) {
    let ws = snapshot(w).workspaces[0].id.clone();
    let mut r = run(dir, &ws, Caps::default(), false).await;
    r.ask(1, Command::Boot);
    let req = r.backend_next().await;
    assert_eq!(req.command, Command::Boot);
    r.answer(req.id, Event::Booted(Box::new(snapshot(w)))).await;
    assert!(matches!(r.ui_next().await.event, Event::Booted(_)), "a cold start has nothing cached");
    let t = target(w, "general");
    r.ask(2, history(&t, None));
    let req = r.backend_next().await;
    r.answer(req.id, Event::History(Box::new(page(w, &t, None)))).await;
    assert!(matches!(r.ui_next().await.event, Event::History(_)));
    r.settled().await;
    r.no_faults();
}

#[tokio::test]
async fn a_warm_start_is_answered_from_the_cache_before_the_backend_is_asked() {
    let (dir, w) = (scratch("warm"), world());
    first_run(&dir, &w).await;
    let ws = snapshot(&w).workspaces[0].id.clone();
    let mut r = run(&dir, &ws, Caps::default(), false).await;
    r.ask(10, Command::Boot);
    // By the time the backend is asked, the cache's answer is already the UI's.
    let req = r.backend_next().await;
    assert_eq!((req.id, &req.command), (Generation(10), &Command::Boot));
    let cached = r.ui.rx.try_recv().expect("the cached boot came before the backend was asked");
    assert_eq!(cached.generation, Generation(10));
    assert_eq!(cached.event, Event::CachedBoot(Box::new(snapshot(&w))));
    // The backend's own answer follows, and replaces the cache's.
    let mut fresh = snapshot(&w);
    fresh.taken = Ts(fresh.taken.0 + 1);
    r.answer(req.id, Event::Booted(Box::new(fresh.clone()))).await;
    assert_eq!(r.ui_next().await.event, Event::Booted(Box::new(fresh.clone())));
    let t = target(&w, "general");
    r.ask(11, history(&t, None));
    let req = r.backend_next().await;
    let cached = r.ui.rx.try_recv().expect("the cached page came before the backend was asked");
    let Event::CachedHistory(p) = cached.event else { panic!("{cached:?}") };
    assert_eq!((cached.generation, p.messages.clone()), (Generation(11), page(&w, &t, None).messages));
    assert!(!p.complete);
    r.answer(req.id, Event::History(Box::new(page(&w, &t, None)))).await;
    assert!(matches!(r.ui_next().await.event, Event::History(_)));
    r.settled().await;
    r.no_faults();
    drop(r);
    // The backend's newer boot is what the third run gets.
    let mut r = run(&dir, &ws, Caps::default(), false).await;
    r.ask(1, Command::Boot);
    assert_eq!(r.ui_next().await.event, Event::CachedBoot(Box::new(fresh)));
}

#[tokio::test]
async fn another_workspaces_cache_and_older_pages_are_never_given_first() {
    let (dir, w) = (scratch("other"), world());
    first_run(&dir, &w).await;
    let mut r = run(&dir, &WorkspaceId::new("TSOMEONEELSE"), Caps::default(), false).await;
    r.ask(1, Command::Boot);
    r.backend_next().await;
    let t = target(&w, "general");
    r.ask(2, history(&t, None));
    r.backend_next().await;
    assert!(r.ui.rx.try_recv().is_err(), "nothing cached of another sign-in");
    drop(r);
    let ws = snapshot(&w).workspaces[0].id.clone();
    let mut r = run(&dir, &ws, Caps::default(), false).await;
    let first = page(&w, &t, None).messages[0].ts;
    r.ask(3, history(&t, Some(first)));
    let req = r.backend_next().await;
    assert!(r.ui.rx.try_recv().is_err(), "an older page is the backend's to give");
    // It is kept all the same.
    r.answer(req.id, Event::History(Box::new(page(&w, &t, Some(first))))).await;
    r.ui_next().await;
    r.settled().await;
    r.ask(4, history(&t, None));
    r.backend_next().await;
    let Event::CachedHistory(p) = r.ui_next().await.event else { panic!("cached") };
    assert_eq!(p.messages, page(&w, &t, None).messages, "still the newest 28");
}

#[tokio::test]
async fn a_failed_answer_goes_through_and_keeps_nothing() {
    let (dir, w) = (scratch("failed"), world());
    first_run(&dir, &w).await;
    let ws = snapshot(&w).workspaces[0].id.clone();
    let mut r = run(&dir, &ws, Caps::default(), false).await;
    r.ask(1, Command::Boot);
    let req = r.backend_next().await;
    assert!(matches!(r.ui_next().await.event, Event::CachedBoot(_)));
    r.backend.tx.send(Envelope::failed(req.id, FailKind::Offline, "off")).await.unwrap();
    assert!(matches!(r.ui_next().await.event, Event::Failed { kind: FailKind::Offline, .. }));
    r.settled().await;
    drop(r);
    let mut r = run(&dir, &ws, Caps::default(), false).await;
    r.ask(1, Command::Boot);
    assert_eq!(r.ui_next().await.event, Event::CachedBoot(Box::new(snapshot(&w))), "the cache is as it was");
}

#[tokio::test]
async fn a_backend_that_takes_no_more_fails_the_request_and_a_backend_gone_ends_the_layer() {
    let (dir, w) = (scratch("gone"), world());
    let ws = snapshot(&w).workspaces[0].id.clone();
    let Run { mut ui, backend, cache, .. } = run(&dir, &ws, Caps::default(), false).await;
    let BackendEnd { rx, tx } = backend;
    drop(rx);
    ui.send(Request { id: Generation(1), workspace: None, command: Command::Boot }).unwrap();
    let e = tokio::time::timeout(Duration::from_secs(5), ui.rx.recv()).await.unwrap().expect("an answer");
    assert_eq!(e.generation, Generation(1));
    assert!(matches!(e.event, Event::Failed { kind: FailKind::Offline, .. }), "{e:?}");
    drop(tx);
    let end = tokio::time::timeout(Duration::from_secs(5), ui.rx.recv()).await.unwrap();
    assert!(end.is_none(), "the layer ends with its backend: {end:?}");
    drop(cache);
}

#[tokio::test]
async fn a_second_slakio_reads_the_cache_and_writes_nothing() {
    let (dir, w) = (scratch("reader"), world());
    first_run(&dir, &w).await;
    let ws = snapshot(&w).workspaces[0].id.clone();
    let writer = run(&dir, &ws, Caps::default(), false).await;
    let mut r = run(&dir, &ws, Caps::default(), true).await;
    assert!(r.cache.read_only());
    r.ask(1, Command::Boot);
    let req = r.backend_next().await;
    assert!(matches!(r.ui_next().await.event, Event::CachedBoot(_)), "it reads");
    let mut other = snapshot(&w);
    other.taken = Ts(1);
    r.answer(req.id, Event::Booted(Box::new(other))).await;
    r.ui_next().await;
    r.settled().await;
    r.no_faults();
    drop(r);
    drop(writer);
    let mut r = run(&dir, &ws, Caps::default(), false).await;
    r.ask(1, Command::Boot);
    assert_eq!(r.ui_next().await.event, Event::CachedBoot(Box::new(snapshot(&w))), "the reader wrote nothing");
}

#[tokio::test]
async fn a_conversation_asked_for_during_the_run_is_never_pruned() {
    let (dir, w) = (scratch("protect"), world());
    first_run(&dir, &w).await;
    let ws = snapshot(&w).workspaces[0].id.clone();
    // A cap so small that everything not protected must go.
    let caps = Caps { max_bytes: 1, ..Caps::default() };
    let mut r = run(&dir, &ws, caps, false).await;
    let shown = target(&w, "general");
    r.ask(1, history(&shown, None));
    let req = r.backend_next().await;
    assert!(matches!(r.ui_next().await.event, Event::CachedHistory(_)));
    r.answer(req.id, Event::History(Box::new(page(&w, &shown, None)))).await;
    r.ui_next().await;
    let others = ["random", "announcements", "frontend"].map(|n| target(&w, n));
    for (i, t) in (0..layer::PRUNE_EVERY).zip(others.iter().cycle()) {
        r.ask(100 + u64::from(i), history(t, None));
        let req = r.backend_next().await;
        r.answer(req.id, Event::History(Box::new(page(&w, t, None)))).await;
        while !matches!(r.ui_next().await.event, Event::History(_)) {}
    }
    r.settled().await;
    // The prune ran after the 32nd page, with the cap at one byte: what was asked for in this
    // run is all still there.
    for (i, t) in std::iter::once(&shown).chain(&others).enumerate() {
        r.ask(500 + i as u64, history(t, None));
        r.backend_next().await;
        assert!(matches!(r.ui_next().await.event, Event::CachedHistory(_)), "{t:?} was pruned");
    }
    drop(r);
    // The next run has asked for nothing yet: its prune takes them.
    let r = run(&dir, &ws, caps, false).await;
    let pruned = r.cache.pruned().await.unwrap();
    assert!(pruned.conversations >= 4, "with nothing protected, the next prune takes them: {pruned:?}");
    drop(r);
    let mut r = run(&dir, &ws, Caps::default(), false).await;
    r.ask(1, history(&shown, None));
    r.backend_next().await;
    assert!(r.ui.rx.try_recv().is_err(), "pruned once nothing protected it");
}

#[tokio::test]
async fn pages_of_a_run_are_kept_from_the_size_prune_that_follows_them() {
    let (dir, w) = (scratch("protect-size"), world());
    first_run(&dir, &w).await;
    let ws = snapshot(&w).workspaces[0].id.clone();
    let mut r = run(&dir, &ws, Caps { max_bytes: 1, ..Caps::default() }, false).await;
    let shown = target(&w, "random");
    for i in 0..layer::PRUNE_EVERY {
        r.ask(10 + u64::from(i), history(&shown, None));
        let req = r.backend_next().await;
        r.answer(req.id, Event::History(Box::new(page(&w, &shown, None)))).await;
        while !matches!(r.ui_next().await.event, Event::History(_)) {}
    }
    r.settled().await;
    r.ask(99, history(&shown, None));
    r.backend_next().await;
    let Event::CachedHistory(p) = r.ui_next().await.event else { panic!("still cached") };
    assert_eq!(p.messages, page(&w, &shown, None).messages, "the prune after 32 pages left the shown one alone");
    // `general`, kept by the first run and not asked for in this one, went.
    let general = target(&w, "general");
    r.ask(100, history(&general, None));
    r.backend_next().await;
    assert!(r.ui.rx.try_recv().is_err(), "{:?}", r.ui.rx.try_recv());
}

#[tokio::test]
async fn a_busy_cache_never_holds_a_request_back() {
    let (dir, w) = (scratch("busy"), world());
    first_run(&dir, &w).await;
    let ws = snapshot(&w).workspaces[0].id.clone();
    let store = open(&dir, false).await;
    // The cache's thread is held in a write (its clock does not answer) until the test lets go.
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = Mutex::new(held);
    let clock = Box::new(move || {
        let _ = held.lock().unwrap().recv_timeout(Duration::from_secs(20));
        0
    });
    let cache = Cache::start(store, Caps::default(), clock, Box::new(|_| {})).unwrap();
    let (inner, mut backend) = BackendHandle::pair(CAPS, 64);
    let slow: Arc<Mutex<Vec<String>>> = Arc::default();
    let said = Arc::clone(&slow);
    let on_slow = Box::new(move |f: &slakio_core::fault::Fault| said.lock().unwrap().push(f.to_string()));
    let mut ui = layer::serve(inner, cache.clone(), ws.clone(), layer::Waits::default(), on_slow);
    let t = target(&w, "general");
    cache.put_page(page(&w, &t, None), None);
    let started = std::time::Instant::now();
    for (id, command) in [(1, Command::Boot), (2, history(&t, None))] {
        ui.send(Request { id: Generation(id), workspace: None, command: command.clone() }).unwrap();
        let req = tokio::time::timeout(Duration::from_secs(2), backend.rx.recv())
            .await
            .expect("handed on while the cache is busy")
            .unwrap();
        assert_eq!((req.id, req.command), (Generation(id), command));
    }
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    assert!(ui.rx.try_recv().is_err(), "nothing cached was given: the cache did not answer in time");
    let said = slow.lock().unwrap().clone();
    assert_eq!(said.len(), 2, "one line for each read not waited for: {said:?}");
    assert!(said[0].contains("boot snapshot") && said[0].contains("500 ms"), "{said:?}");
    assert!(said[1].contains("newest page") && said[1].contains("100 ms"), "{said:?}");
    // The backend's answers go through as ever.
    backend
        .tx
        .send(Envelope { generation: Generation(1), event: Event::Booted(Box::new(snapshot(&w))) })
        .await
        .unwrap();
    let got = tokio::time::timeout(Duration::from_secs(5), ui.rx.recv()).await.unwrap().unwrap();
    assert!(matches!(got.event, Event::Booted(_)), "{got:?}");
    drop(release);
}

/// A second front, for workspace `workspace`, on the same cache as `r`.
fn second_front(r: &Run, workspace: &WorkspaceId) -> (BackendHandle, BackendEnd) {
    let (inner, backend) = BackendHandle::pair(CAPS, 64);
    let ui = layer::serve(inner, r.cache.clone(), workspace.clone(), layer::Waits::default(), Box::new(|_| {}));
    (ui, backend)
}

#[tokio::test]
async fn two_workspaces_share_one_cache_and_each_keeps_its_own_part() {
    let (dir, w) = (scratch("two"), world());
    let (a, b) = (w.snapshot().workspaces[0].id.clone(), w.snapshot().workspaces[1].id.clone());
    let mut r = run(&dir, &a, Caps::default(), false).await;
    // Each workspace claimed through the cache's thread, for its own user.
    let wait = Duration::from_secs(5);
    let c = r.cache.clone();
    let claims =
        tokio::task::spawn_blocking(move || (c.claim(a.as_str(), "UA", wait), c.claim(b.as_str(), "UB", wait)));
    let (a_claim, b_claim) = claims.await.unwrap();
    assert!(matches!((a_claim, b_claim), (Some(Ok(true)), Some(Ok(true)))));
    let (a, b) = (w.snapshot().workspaces[0].id.clone(), w.snapshot().workspaces[1].id.clone());
    let (mut ui_b, mut end_b) = second_front(&r, &b);
    // Both boot; each answer is of its own workspace (B's backend even names A's data: not kept).
    r.ask(1, Command::Boot);
    ui_b.send(Request { id: Generation(2), workspace: Some(b.clone()), command: Command::Boot }).unwrap();
    let req_a = r.backend_next().await;
    let req_b = end_b.rx.recv().await.unwrap();
    r.answer(req_a.id, Event::Booted(Box::new(w.snapshot().only(&a)))).await;
    let whole = w.snapshot().clone();
    end_b.tx.send(Envelope { generation: req_b.id, event: Event::Booted(Box::new(whole)) }).await.unwrap();
    r.ui_next().await;
    ui_b.rx.recv().await.unwrap();
    // A boots again, smaller: B's part is not touched.
    r.ask(3, Command::Boot);
    let again = r.backend_next().await;
    let mut smaller = w.snapshot().only(&a);
    smaller.conversations.truncate(1);
    r.answer(again.id, Event::Booted(Box::new(smaller.clone()))).await;
    while !matches!(r.ui_next().await.event, Event::Booted(_)) {}
    r.settled().await;
    r.no_faults();
    drop((r, ui_b, end_b));
    let store = open(&dir, false).await;
    assert_eq!(store.snapshot(std::slice::from_ref(&a)).unwrap(), Some(smaller));
    assert_eq!(store.snapshot(std::slice::from_ref(&b)).unwrap(), Some(w.snapshot().only(&b)), "B's own part");
}

#[tokio::test]
async fn a_conversation_one_workspace_shows_is_kept_from_another_workspaces_prune() {
    let (dir, w) = (scratch("two-protect"), world());
    first_run(&dir, &w).await;
    let a = w.snapshot().workspaces[0].id.clone();
    let b = w.snapshot().workspaces[1].id.clone();
    let mut r = run(&dir, &a, Caps { max_bytes: 1, ..Caps::default() }, false).await;
    let shown = target(&w, "general");
    r.ask(1, history(&shown, None));
    r.backend_next().await;
    assert!(matches!(r.ui_next().await.event, Event::CachedHistory(_)));
    // B's front boots and prunes (after its first boot) with a one-byte cap.
    let (mut ui_b, mut end_b) = second_front(&r, &b);
    ui_b.send(Request { id: Generation(2), workspace: Some(b.clone()), command: Command::Boot }).unwrap();
    let req = end_b.rx.recv().await.unwrap();
    end_b
        .tx
        .send(Envelope { generation: req.id, event: Event::Booted(Box::new(w.snapshot().only(&b))) })
        .await
        .unwrap();
    while !matches!(ui_b.rx.recv().await.unwrap().event, Event::Booted(_)) {}
    r.settled().await;
    r.ask(3, history(&shown, None));
    r.backend_next().await;
    assert!(matches!(r.ui_next().await.event, Event::CachedHistory(_)), "A's open conversation outlived B's prune");
}

#[tokio::test]
async fn a_thread_s_newest_page_is_kept_and_given_first_on_a_later_open() {
    let (dir, w) = (scratch("thread"), world());
    first_run(&dir, &w).await;
    let ws = snapshot(&w).workspaces[0].id.clone();
    let conv = target(&w, "general");
    let parent = page(&w, &conv, None).messages[0].clone();
    let thread = Target::Thread { workspace: ws.clone(), conversation: conv.conversation().clone(), thread: parent.ts };
    let replies =
        Page { target: thread.clone(), messages: vec![parent.clone()], complete: true, users: Vec::new(), cut: false };
    let mut r = run(&dir, &ws, Caps::default(), false).await;
    r.ask(1, history(&thread, None));
    let req = r.backend_next().await;
    assert!(r.ui.rx.try_recv().is_err(), "nothing kept of it yet");
    r.answer(req.id, Event::History(Box::new(replies.clone()))).await;
    r.ui_next().await;
    r.settled().await;
    r.no_faults();
    drop(r);
    let mut r = run(&dir, &ws, Caps::default(), false).await;
    r.ask(2, history(&thread, None));
    r.backend_next().await;
    let Event::CachedHistory(p) = r.ui_next().await.event else { panic!("cached") };
    assert_eq!((p.target, p.messages), (thread, replies.messages), "the thread, not its conversation");
}

#[tokio::test]
async fn a_thread_read_only_in_part_is_not_kept() {
    let (dir, w) = (scratch("thread-cut"), world());
    first_run(&dir, &w).await;
    let ws = snapshot(&w).workspaces[0].id.clone();
    let conv = target(&w, "general");
    let parent = page(&w, &conv, None).messages[0].clone();
    let thread = Target::Thread { workspace: ws.clone(), conversation: conv.conversation().clone(), thread: parent.ts };
    let cut = Page { target: thread.clone(), messages: vec![parent], complete: true, users: Vec::new(), cut: true };
    let mut r = run(&dir, &ws, Caps::default(), false).await;
    r.ask(1, history(&thread, None));
    let req = r.backend_next().await;
    r.answer(req.id, Event::History(Box::new(cut))).await;
    assert!(matches!(r.ui_next().await.event, Event::History(_)), "passed on as it is");
    r.settled().await;
    r.no_faults();
    drop(r);
    let mut r = run(&dir, &ws, Caps::default(), false).await;
    r.ask(2, history(&thread, None));
    r.backend_next().await;
    assert!(r.ui.rx.try_recv().is_err(), "no word on what is newer: nothing of it kept");
}
