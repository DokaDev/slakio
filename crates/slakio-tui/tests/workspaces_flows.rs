//! Two workspaces signed in at once, each behind a backend of its own and one handle in front of
//! them all (`many`): each boots on its own; one that fails, hangs or is gone leaves the other
//! listed and working, with its own "Could not load" row and words; a retry asks only it again;
//! a change goes only where it can be made; the switcher and counts follow the real workspaces.
//! Driven the way the binary's loop drives a handle (`exchange::pump`), on a Tokio runtime.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use slakio_core::backend::{
    BackendEnd, BackendHandle, Capabilities, Command, Envelope, Event, FailKind, Generation, Request, SavedChange,
};
use slakio_core::i18n::Lang;
use slakio_core::model::{ConversationId, SavedId, Target, WorkspaceId};
use slakio_tui::app::model::{Row, SignedIn};
use slakio_tui::app::{App, Settings};
use slakio_tui::demo::DemoBackend;
use slakio_tui::keymap::parse_keys;
use slakio_tui::theme::Theme;
use slakio_tui::{exchange, many};
use slakio_world::World;
use std::time::{Duration, Instant};

const READ_ONLY: Capabilities = Capabilities { demo: false, read_only: true };

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

fn ids() -> (WorkspaceId, WorkspaceId) {
    let w = World::demo();
    let s = w.snapshot();
    (s.workspaces[0].id.clone(), s.workspaces[1].id.clone())
}

/// A workspace's backend over the demo world, as read-only (or not) as asked.
fn world(read_only: bool) -> BackendHandle {
    let mut h = exchange::serve(Box::new(DemoBackend::new(World::demo())));
    h.capabilities = Capabilities { demo: false, read_only };
    h
}

/// A backend that answers every request with `kind`.
fn failing(kind: FailKind) -> BackendHandle {
    let (h, mut end) = BackendHandle::pair(READ_ONLY, 8);
    tokio::spawn(async move {
        while let Some(r) = end.rx.recv().await {
            if end.tx.send(Envelope::failed(r.id, kind.clone(), "test")).await.is_err() {
                return;
            }
        }
    });
    h
}

/// A backend that takes requests and never answers; its end is handed back to keep it alive.
fn silent(bound: usize) -> (BackendHandle, BackendEnd) {
    BackendHandle::pair(READ_ONLY, bound)
}

/// The app at `w`×`h` over the handle of `backends` (A then B), signed in to both.
struct Two {
    d: Demo,
    handle: BackendHandle,
}

impl Two {
    fn new(w: u16, h: u16, a: BackendHandle, b: BackendHandle) -> Self {
        Self::named(w, h, a, b, Lang::En, ["A company", "B side"])
    }

    /// [`Two::new`] in `lang`, the workspaces named `names`.
    fn named(w: u16, h: u16, a: BackendHandle, b: BackendHandle, lang: Lang, names: [&str; 2]) -> Self {
        let (ia, ib) = ids();
        let read_only = (a.capabilities.read_only, b.capabilities.read_only);
        let handle = many::serve(vec![(ia.clone(), a), (ib.clone(), b)]);
        let mut d = Demo::new(w, h);
        let mut app = App::new(lang, Theme::terminal());
        app.settings = Settings::default();
        app.resize(w, h);
        let signed = [
            SignedIn { id: ia, name: names[0].into(), read_only: read_only.0 },
            SignedIn { id: ib, name: names[1].into(), read_only: read_only.1 },
        ];
        app.connect_workspaces(handle.capabilities, &signed);
        d.app = app;
        Self { d, handle }
    }

    /// Hand the app's requests over and take every answer that comes within a short while.
    async fn settle(&mut self) {
        exchange::pump(&mut self.d.app, &mut self.handle).await;
        while let Ok(Some(e)) = tokio::time::timeout(Duration::from_millis(150), self.handle.rx.recv()).await {
            self.d.app.on_backend(e, Instant::now());
            exchange::pump(&mut self.d.app, &mut self.handle).await;
        }
    }

    async fn keys(&mut self, notation: &str) {
        for k in parse_keys(notation).unwrap() {
            self.d.app.handle_event(ratatui::crossterm::event::Event::Key(k.to_event()), self.d.now);
        }
        self.settle().await;
    }

    fn names(&self, i: usize) -> Vec<String> {
        let m = &self.d.app.model;
        let ws = &m.workspaces()[i].id;
        m.conversations().iter().filter(|c| &c.workspace == ws).map(|c| c.id.to_string()).collect()
    }
}

#[test]
fn both_workspaces_boot_on_their_own_and_the_switcher_shows_their_counts() {
    rt().block_on(async {
        let mut t = Two::new(120, 40, world(true), world(true));
        let whole = World::demo();
        t.settle().await;
        for i in 0..2 {
            let ws = &whole.snapshot().workspaces[i].id;
            let want: Vec<String> = whole
                .snapshot()
                .conversations
                .iter()
                .filter(|c| &c.workspace == ws)
                .map(|c| c.id.to_string())
                .collect();
            assert_eq!(t.names(i), want, "workspace {i} lists its own conversations");
        }
        // Each boot asked for its workspace alone.
        assert!(t.d.app.model.is_loaded_in(&ids().0) && t.d.app.model.is_loaded_in(&ids().1));
        // Space W opens the switcher on A; j, Enter: B, from its Home.
        t.keys("space W").await;
        assert_eq!(t.d.app.switcher(), Some(0));
        let shown = t.d.screen();
        assert!(shown.contains("A company") && shown.contains("B side"), "{shown}");
        t.keys("j enter").await;
        assert_eq!(t.d.app.workspace(), 1);
        let b = &ids().1;
        let rows = t.d.app.list_rows();
        assert!(rows.iter().all(|r| match r {
            Row::Conversation(i) => &t.d.app.model.conversation(*i).workspace == b,
            _ => true,
        }));
        insta::assert_snapshot!("workspaces_two_b_shown_120x40", t.d.snap());
    });
}

#[test]
fn a_workspace_that_fails_leaves_the_other_listed_with_its_own_row_and_words() {
    rt().block_on(async {
        let mut t = Two::new(120, 40, world(true), failing(FailKind::Auth));
        t.settle().await;
        let (a, b) = ids();
        assert!(t.d.app.model.is_loaded_in(&a), "A is listed");
        assert!(!t.d.app.model.is_loaded_in(&b));
        assert_eq!(t.d.app.boot_failed_in(&a), None);
        assert_eq!(t.d.app.boot_failed_in(&b), Some(&FailKind::Auth));
        assert_eq!(t.d.app.boot_failed(), None, "A is shown: nothing failed here");
        let status = t.d.status_line();
        assert!(status.contains("B side: Signed out"), "the words say whose start failed: {status}");
        t.keys("space W j enter").await;
        let shown = t.d.screen();
        assert!(shown.contains("│ Could not load "), "{shown}");
        insta::assert_snapshot!("workspaces_b_failed_120x40", t.d.snap());
        // Back on A: everything of it is there.
        t.keys("space W k enter").await;
        assert!(!t.d.screen().contains("Could not load"));
        assert!(!t.names(0).is_empty());
    });
}

#[test]
fn a_workspace_slack_refuses_says_why_and_what_to_do_after_its_notice_is_gone() {
    rt().block_on(async {
        for (lang, w, h) in
            [(Lang::En, 120, 40), (Lang::En, 100, 30), (Lang::En, 80, 24), (Lang::Ko, 120, 40), (Lang::Ko, 80, 24)]
        {
            let names = ["A company", "Acme Engineering EU"];
            let mut t = Two::named(w, h, world(true), failing(FailKind::Auth), lang, names);
            t.settle().await;
            // The notice keeps its reason whole: the name gives way.
            let status = t.d.status_line();
            let why = t.d.app.i18n.label(slakio_core::i18n::Label::FailAuth).to_string();
            assert!(status.contains(&why), "{lang:?} {w}x{h}: the whole reason: {status}");
            assert!(status.contains("slakio login"), "{status}");
            t.keys("space W").await;
            // The switcher marks it, where its count would be.
            let mark = t.d.app.i18n.label(slakio_core::i18n::Label::SwitcherSignedOut).to_string();
            let row = t.d.screen().lines().find(|l| l.contains("Acme")).unwrap().to_string();
            assert!(row.contains(&mark), "{lang:?} {w}x{h}: {row}");
            t.keys("j enter").await;
            // Long after the notice: the list still says why, and what to do.
            t.d.now = Instant::now() + Duration::from_secs(7);
            let shown = t.d.screen();
            let status = t.d.status_line();
            assert!(!status.contains(&why), "the notice is gone: {status}");
            let list: String =
                shown.lines().map(|l| l.chars().take(30).collect::<String>()).collect::<Vec<_>>().join("\n");
            assert!(list.contains("slakio"), "{lang:?} {w}x{h}: {shown}");
            assert!(list.contains("login"), "{lang:?} {w}x{h}: {shown}");
            // Asking again cannot help, and its views hold nothing: neither is offered.
            let (retry, views) = (
                t.d.app.i18n.label(slakio_core::i18n::Label::HintRetry).to_string(),
                t.d.app.i18n.label(slakio_core::i18n::Label::HintNav).to_string(),
            );
            assert!(!status.contains(&format!("r {retry}")) && !status.contains(&format!("[/] {views}")), "{status}");
            if (lang, w) == (Lang::En, 120) {
                assert!(shown.contains("│ Could not load "), "{shown}");
                assert!(shown.contains("│ Signed out: run slakio "), "{shown}");
            }
        }
    });
}

#[test]
fn a_hung_or_gone_workspace_holds_nothing_back() {
    rt().block_on(async {
        let (b, end) = silent(8);
        let mut t = Two::new(80, 24, world(true), b);
        t.settle().await;
        let (ia, ib) = ids();
        assert!(t.d.app.model.is_loaded_in(&ia), "A answered while B never does");
        assert!(!t.d.app.model.is_loaded_in(&ib));
        t.keys("space W j enter").await;
        assert!(t.d.screen().contains("Loading"), "B is still loading: {}", t.d.screen());
        insta::assert_snapshot!("workspaces_b_loading_80x24", t.d.snap());
        // B's backend goes away: its retry fails at once as offline, A still opens.
        drop(end);
        tokio::task::yield_now().await;
        t.keys("r").await;
        assert_eq!(t.d.app.boot_failed_in(&ib), Some(&FailKind::Offline), "B's retry fails as offline");
        assert!(t.d.screen().contains("Could not load"), "{}", t.d.screen());
        assert!(t.d.app.model.is_loaded_in(&ia), "A is still listed");
        t.keys("space W k enter").await;
        let rows = t.d.app.list_rows();
        let first = rows.iter().position(|r| matches!(r, Row::Conversation(_))).unwrap();
        t.d.app.focus_list_row(first);
        t.keys("enter").await;
        let pane = t.d.app.focused_pane().expect("A's conversation opened");
        assert_eq!(pane.target().workspace(), &ia);
    });
}

#[test]
fn a_retry_asks_again_for_the_workspace_shown_only() {
    rt().block_on(async {
        // B fails once (a network error, which may pass), then works; after the retry's answer,
        // a late answer to the first ask comes (a list without B's conversations), then a late
        // failure of it.
        let (h, mut end) = BackendHandle::pair(READ_ONLY, 8);
        tokio::spawn(async move {
            let mut demo = exchange::serve(Box::new(DemoBackend::new(World::demo())));
            let mut first = None;
            while let Some(r) = end.rx.recv().await {
                let Some(stale) = first else {
                    first = Some(r.id);
                    let _ = end.tx.send(Envelope::failed(r.id, FailKind::Network, "test")).await;
                    continue;
                };
                demo.tx.send(r).await.unwrap();
                let e = demo.rx.recv().await.unwrap();
                let _ = end.tx.send(e).await;
                let mut old = World::demo().snapshot().only(&ids().1);
                old.conversations.clear();
                let _ = end.tx.send(Envelope { generation: stale, event: Event::Booted(Box::new(old)) }).await;
                let _ = end.tx.send(Envelope::failed(stale, FailKind::Network, "late")).await;
            }
        });
        let a_boots = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (a, mut a_end) = BackendHandle::pair(READ_ONLY, 8);
        let counted = std::sync::Arc::clone(&a_boots);
        tokio::spawn(async move {
            let mut demo = exchange::serve(Box::new(DemoBackend::new(World::demo())));
            while let Some(r) = a_end.rx.recv().await {
                if r.command == Command::Boot {
                    counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
                demo.tx.send(r).await.unwrap();
                let e = demo.rx.recv().await.unwrap();
                let _ = a_end.tx.send(e).await;
            }
        });
        let mut t = Two::new(120, 40, a, h);
        t.settle().await;
        assert_eq!(t.d.app.boot_failed_in(&ids().1), Some(&FailKind::Network));
        t.keys("space W j enter").await;
        t.keys("r").await;
        assert_eq!(t.d.app.boot_failed_in(&ids().1), None, "B came back; the first ask's late failure is dropped");
        assert!(t.d.app.model.is_loaded_in(&ids().1));
        assert!(!t.names(1).is_empty(), "the first ask's late list is dropped");
        assert_eq!(a_boots.load(std::sync::atomic::Ordering::SeqCst), 1, "A was not asked again");
    });
}

#[test]
fn read_only_is_per_workspace_and_a_change_goes_only_where_it_can_be_made() {
    rt().block_on(async {
        let mut t = Two::new(120, 40, world(true), world(false));
        t.settle().await;
        assert!(!t.handle.capabilities.read_only, "one of them can change things");
        assert!(t.d.app.read_only(), "A, shown, is read-only");
        t.keys("space W j enter").await;
        assert!(!t.d.app.read_only(), "B is not");
        // A change for A is refused by A's own handle; nothing reaches A's world.
        let (ia, ib) = ids();
        let save = |id: u64, ws: &WorkspaceId| Request {
            id: Generation(10_000 + id),
            workspace: Some(ws.clone()),
            command: Command::Save { item: SavedId::new("S-none"), change: SavedChange::Complete },
        };
        t.handle.tx.send(save(1, &ia)).await.unwrap();
        let e = t.handle.rx.recv().await.unwrap();
        assert!(matches!(e.event, Event::Failed { kind: FailKind::ReadOnly, .. }), "{e:?}");
        t.handle.tx.send(save(2, &ib)).await.unwrap();
        let e = t.handle.rx.recv().await.unwrap();
        assert!(matches!(e.event, Event::Saved { .. }), "B carries it out: {e:?}");
    });
}

#[test]
fn a_request_for_no_workspace_or_one_not_signed_in_goes_nowhere() {
    rt().block_on(async {
        let (b, mut b_end) = silent(8);
        let h = many::serve(vec![(ids().1, b)]);
        let ask = |id: u64, workspace: Option<WorkspaceId>, command: Command| Request {
            id: Generation(id),
            workspace,
            command,
        };
        h.tx.send(ask(1, None, Command::Boot)).await.unwrap();
        h.tx.send(ask(2, Some(ids().0), Command::Boot)).await.unwrap();
        let target = Target::Conversation { workspace: ids().0, conversation: ConversationId::new("C1") };
        h.tx.send(ask(3, Some(ids().1), Command::History { target, before: None, limit: 5 })).await.unwrap();
        let mut h = h;
        for want in 1..=3 {
            let e = h.rx.recv().await.unwrap();
            assert_eq!(e.generation, Generation(want));
            assert!(matches!(e.event, Event::Failed { kind: FailKind::Auth, .. }), "{e:?}");
        }
        // A history request goes by its target's workspace, whatever the request names.
        assert!(b_end.rx.try_recv().is_err(), "nothing reached B");
    });
}

#[test]
fn a_full_queue_of_one_workspace_is_busy_for_it_alone() {
    rt().block_on(async {
        let (b, _b_end) = silent(1);
        let mut h = many::serve(vec![(ids().0, world(true)), (ids().1, b)]);
        let boot =
            |id: u64, ws: WorkspaceId| Request { id: Generation(id), workspace: Some(ws), command: Command::Boot };
        h.tx.send(boot(1, ids().1)).await.unwrap();
        h.tx.send(boot(2, ids().1)).await.unwrap();
        h.tx.send(boot(3, ids().0)).await.unwrap();
        let mut got = Vec::new();
        for _ in 0..2 {
            got.push(tokio::time::timeout(Duration::from_secs(5), h.rx.recv()).await.unwrap().unwrap());
        }
        got.sort_by_key(|e| e.generation);
        assert!(matches!(got[0].event, Event::Failed { kind: FailKind::Busy, .. }), "{:?}", got[0]);
        assert_eq!(got[0].generation, Generation(2));
        assert!(matches!(&got[1].event, Event::Booted(s) if s.workspaces.len() == 1 && s.workspaces[0].id == ids().0));
    });
}
