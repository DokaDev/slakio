//! The backend protocol end to end over a scripted backend: answers that come late, out of
//! order, twice or never; failed requests (nothing keeps waiting for them); a read-only backend
//! (a change is refused before anything is sent, and none is offered); saved items the backend
//! cannot tell (unavailable, not empty); and the binary's handle, where a late answer wakes the
//! loop.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
mod scripted;

use demo::mask_hangul;
use scripted::Scripted;
use slakio_core::backend::{BackendHandle, Capabilities, Command, Envelope, Event, FailKind, Snapshot};
use slakio_core::i18n::Lang;
use slakio_tui::app::App;
use slakio_tui::app::shell::View;
use slakio_tui::demo::DemoBackend;
use slakio_tui::exchange;
use slakio_tui::theme::Theme;
use slakio_world::World;
use std::time::{Duration, Instant};

/// Booted, the conversation `name` opened; its first page asked for and held.
fn opened(name: &str) -> Scripted {
    let mut s = Scripted::booted(false, None);
    s.d.list_cursor_on(name);
    s.keys("enter");
    assert_eq!(s.b.held.len(), 1, "the newest page is asked for");
    assert!(matches!(s.b.held[0].command, Command::History { .. }));
    s
}

#[test]
fn the_list_waits_for_a_late_boot_and_says_so_when_it_fails() {
    let mut s = Scripted::connected(false);
    s.pump();
    assert_eq!(s.b.held.len(), 1);
    assert_eq!(s.b.held[0].workspace, None, "the boot is about every workspace");
    assert!(s.d.screen().contains("Loading"), "{}", s.d.screen());
    s.b.fail(0, FailKind::Network);
    assert!(s.pump());
    let screen = s.d.screen();
    assert!(!screen.contains("Loading"), "never Loading for ever: {screen}");
    assert!(screen.contains("Could not load workspaces"), "{screen}");
    assert!(s.d.status_line().contains("Network error"), "{}", s.d.status_line());
}

#[test]
fn a_page_request_names_its_workspace_and_a_late_answer_still_lands() {
    let mut s = opened("backend");
    assert_eq!(s.b.held[0].workspace.as_ref().map(|w| w.as_str()), Some("TDEMOA"));
    assert!(s.d.screen().contains("Loading messages"));
    // Nothing else happens for a while; then the answer comes.
    assert!(!s.pump(), "no answer, no change");
    s.b.answer(0);
    assert!(s.pump());
    assert!(!s.d.screen().contains("Loading messages"), "{}", s.d.screen());
}

#[test]
fn a_duplicate_or_out_of_order_answer_changes_nothing() {
    let mut s = opened("backend");
    let first = s.b.answer(0);
    assert!(s.pump());
    let before = s.d.screen();
    s.b.push(first.clone());
    assert!(!s.pump(), "the same answer twice");
    // An answer to a request nobody waits for any more (an older id).
    let older = Envelope { generation: slakio_core::backend::Generation(first.generation.0 - 1), ..first };
    s.b.push(older);
    assert!(!s.pump(), "an older answer");
    assert_eq!(s.d.screen(), before);
}

#[test]
fn a_failed_page_ends_loading_with_an_error_row_and_asks_no_more() {
    let mut s = opened("backend");
    s.b.fail(0, FailKind::Offline);
    assert!(s.pump());
    let screen = s.d.screen();
    assert!(screen.contains("Could not load messages"), "{screen}");
    assert!(!screen.contains("Loading messages"), "{screen}");
    assert!(s.d.status_line().contains("Offline"), "{}", s.d.status_line());
    assert!(s.b.held.is_empty(), "no request in a loop after a failure");
    // A failure nobody waits for is dropped.
    s.b.push(Envelope::failed(slakio_core::backend::Generation(999), FailKind::Network, ""));
    assert!(!s.pump());
}

/// To-dos shown, the cursor on its first to-do in progress.
fn todos(s: &mut Scripted) {
    s.keys("space o");
    assert_eq!(s.d.app.view(), View::Todos);
    s.keys("j j j");
}

#[test]
fn a_failed_change_stops_waiting_and_says_why() {
    let mut s = Scripted::booted(false, None);
    todos(&mut s);
    let before = s.d.screen();
    s.keys("x");
    assert_eq!(s.b.held.len(), 1);
    assert!(matches!(s.b.held[0].command, Command::Save { .. }));
    assert!(s.b.held[0].workspace.is_some(), "a change names its workspace");
    s.b.fail(0, FailKind::RateLimited);
    assert!(s.pump());
    assert!(s.d.status_line().contains("slow down"), "{}", s.d.status_line());
    // Nothing waits any more: a late success for the failed request is dropped.
    let late = s.b.log.len();
    s.keys("x");
    let again = s.b.answer(0);
    assert_eq!(s.b.log.len(), late + 1);
    assert!(s.pump());
    s.b.push(Envelope { generation: slakio_core::backend::Generation(again.generation.0 - 1), ..again });
    assert!(!s.pump(), "the failed request's id is not waited for");
    assert_ne!(s.d.screen(), before, "the second change landed");
}

#[test]
fn a_read_only_backend_refuses_a_change_without_sending_it() {
    let mut s = Scripted::booted(true, None);
    todos(&mut s);
    let before: Vec<_> = s.d.app.list_rows();
    let log = s.b.log.len();
    for k in ["x", "m", "d"] {
        s.keys(k);
        assert!(s.d.status_line().contains("Read only"), "{k}: {}", s.d.status_line());
    }
    assert!(s.b.held.is_empty() && s.b.log.len() == log, "nothing sent: {:?}", s.b.log);
    assert_eq!(s.d.app.list_rows(), before, "nothing changed");
    assert!(s.d.app.overlay().is_none(), "d asks nothing");
}

#[test]
fn a_read_only_backend_answers_a_change_sent_anyway_with_read_only() {
    let mut b = scripted::ScriptedBackend::new(true);
    use slakio_core::backend::{Backend, Request, SavedChange};
    let item = World::demo().saved()[0].clone();
    let save = Command::Save { item: item.id, change: SavedChange::Complete };
    b.send(Request { id: slakio_core::backend::Generation(7), workspace: Some(item.workspace), command: save });
    let answer = b.poll().expect("answered at once");
    assert!(matches!(answer.event, Event::Failed { kind: FailKind::ReadOnly, .. }));
    assert_eq!(answer.generation.0, 7);
    assert!(b.log.is_empty() && b.held.is_empty(), "the request went nowhere");
}

#[test]
fn a_read_only_backend_offers_no_change() {
    let mut s = Scripted::booted(true, None);
    todos(&mut s);
    let status = s.d.status_line();
    assert!(!status.contains("done") && !status.contains("clear") && !status.contains("to Saved"), "{status}");
    insta::assert_snapshot!("todos_read_only_120x40", mask_hangul(&s.d.screen()));
    s.keys(":");
    for c in "todo".chars() {
        s.d.app.handle_event(
            ratatui::crossterm::event::Event::Key(ratatui::crossterm::event::KeyEvent::new(
                ratatui::crossterm::event::KeyCode::Char(c),
                ratatui::crossterm::event::KeyModifiers::NONE,
            )),
            s.d.now,
        );
    }
    let screen = s.d.screen();
    assert!(!screen.contains("Clear the saved item") && !screen.contains("todo.toggle"), "{screen}");
}

#[test]
fn saved_items_the_backend_cannot_tell_are_unavailable_not_empty() {
    let mut snapshot: Snapshot = World::demo().snapshot().clone();
    snapshot.saved = None;
    let mut s = Scripted::booted(false, Some(snapshot));
    s.keys("space o");
    let screen = s.d.screen();
    assert!(screen.contains("Not available"), "{screen}");
    assert!(!screen.contains("Nothing here"), "{screen}");
}

#[test]
fn an_answer_brings_the_backends_clock() {
    let mut s = Scripted::booted(false, None);
    todos(&mut s);
    let overdue = |s: &Scripted| s.d.screen().matches("Overdue").count();
    let before = overdue(&s);
    s.keys("x");
    let mut e = s.b.reply(0);
    let Event::Saved { taken, .. } = &mut e.event else { panic!("a saved answer") };
    // The clock moved on a year: every reminder in progress is overdue now.
    *taken = slakio_core::model::Ts(taken.0 + 365 * 86_400 * 1_000_000);
    let moved = *taken;
    s.b.push(e);
    assert!(s.pump());
    assert_eq!(s.d.app.model.taken(), moved);
    assert!(overdue(&s) > before, "overdue by the new clock: {}", s.d.screen());
}

#[tokio::test]
async fn a_late_answer_wakes_the_loop() {
    let caps = Capabilities { demo: false, read_only: false };
    let (mut handle, mut end) = BackendHandle::pair(caps, 4);
    let mut app = App::new(Lang::En, Theme::terminal());
    app.resize(80, 24);
    app.connect(handle.capabilities);
    assert!(!exchange::pump(&mut app, &mut handle).await, "nothing answered yet");
    let boot = end.rx.recv().await.expect("the boot request went out");
    let snapshot = World::demo().snapshot().clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let _ = end.tx.send(Envelope { generation: boot.id, event: Event::Booted(Box::new(snapshot)) }).await;
        // Kept open until the answer is taken.
        tokio::time::sleep(Duration::from_secs(1)).await;
    });
    let envelope = tokio::time::timeout(Duration::from_secs(5), exchange::next(Some(&mut handle)))
        .await
        .expect("the late answer wakes the loop");
    assert!(app.on_backend(envelope, Instant::now()));
    assert!(app.model.is_loaded());
}

#[tokio::test]
async fn the_demo_behind_its_handle_answers_in_the_same_turn() {
    let mut handle = exchange::serve(Box::new(DemoBackend::new(World::demo())));
    let mut app = App::new(Lang::En, Theme::terminal());
    app.resize(80, 24);
    app.connect(handle.capabilities);
    assert!(exchange::pump(&mut app, &mut handle).await);
    assert!(app.model.is_loaded(), "the first frame is drawn booted");
}

#[tokio::test]
async fn a_backend_that_is_gone_fails_the_request_at_once() {
    let caps = Capabilities { demo: false, read_only: false };
    let (mut handle, end) = BackendHandle::pair(caps, 4);
    drop(end);
    let mut app = App::new(Lang::En, Theme::terminal());
    app.resize(80, 24);
    app.connect(handle.capabilities);
    assert!(exchange::pump(&mut app, &mut handle).await);
    assert_eq!(app.boot_failed(), Some(FailKind::Offline), "never waits for an answer that cannot come");
}

/// The demo world, read-only, counting every change that reaches it.
struct Counting {
    demo: DemoBackend,
    changes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl slakio_core::backend::Backend for Counting {
    fn capabilities(&self) -> Capabilities {
        Capabilities { demo: true, read_only: true }
    }

    fn send(&mut self, request: slakio_core::backend::Request) {
        if request.command.changes() {
            self.changes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        self.demo.send(request);
    }

    fn poll(&mut self) -> Option<Envelope> {
        self.demo.poll()
    }
}

#[tokio::test]
async fn behind_a_read_only_handle_no_change_reaches_the_backend() {
    use slakio_core::backend::{Request, SavedChange};
    let changes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let backend = Counting { demo: DemoBackend::new(World::demo()), changes: changes.clone() };
    let mut handle = exchange::serve(Box::new(backend));
    let mut app = App::new(Lang::En, Theme::terminal());
    app.resize(80, 24);
    app.connect(handle.capabilities);
    assert!(exchange::pump(&mut app, &mut handle).await);
    // Sent past the UI (which offers no change): the handle itself refuses it.
    let item = World::demo().saved()[0].clone();
    let save = Command::Save { item: item.id, change: SavedChange::Complete };
    let refused = handle.send(Request { id: slakio_core::backend::Generation(99), workspace: None, command: save });
    let Err(answer) = refused else { panic!("a change is refused before it is sent") };
    assert!(matches!(answer.event, Event::Failed { kind: FailKind::ReadOnly, .. }));
    tokio::task::yield_now().await;
    assert_eq!(changes.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing reached the backend");
}

#[test]
fn a_failure_goes_to_the_error_log_as_one_safe_line() {
    use slakio_tui::app::Effect;
    let mut s = opened("backend");
    let id = s.b.held.remove(0).id;
    let long = "x".repeat(500);
    s.b.push(Envelope::failed(id, FailKind::Network, format!("reset\u{1b}[31m by\npeer {long}")));
    assert!(s.pump());
    let logged: Vec<_> =
        s.d.app
            .take_effects()
            .into_iter()
            .filter_map(|e| match e {
                Effect::Record { context, fault } => Some((context, fault.detail)),
                _ => None,
            })
            .collect();
    assert_eq!(logged.len(), 1, "{logged:?}");
    let (context, detail) = &logged[0];
    assert_eq!(*context, "backend");
    assert!(detail.starts_with("Network: reset [31m by peer x"), "{detail}");
    assert!(!detail.chars().any(char::is_control), "{detail:?}");
    assert!(detail.chars().count() <= "Network: ".len() + 200, "cut: {}", detail.len());
}

#[test]
fn r_asks_again_for_workspaces_that_failed_for_a_reason_that_may_pass() {
    let mut s = Scripted::connected(false);
    s.pump();
    s.b.fail(0, FailKind::RateLimited);
    s.pump();
    assert!(s.d.screen().contains("r retry"), "{}", s.d.screen());
    assert!(s.b.held.is_empty(), "nothing retries on its own");
    s.keys("r");
    assert_eq!(s.b.held.len(), 1, "asked again");
    assert!(matches!(s.b.held[0].command, Command::Boot));
    assert!(s.d.screen().contains("Loading"), "{}", s.d.screen());
    s.b.answer(0);
    s.pump();
    assert!(s.d.app.model.is_loaded());
}

#[test]
fn r_does_not_ask_again_when_asking_again_cannot_help() {
    let mut s = Scripted::connected(false);
    s.pump();
    s.b.fail(0, FailKind::Auth);
    s.pump();
    assert!(!s.d.screen().contains("r retry"), "{}", s.d.screen());
    s.keys("r");
    assert!(s.b.held.is_empty(), "signed out: asking again cannot help");
    assert!(s.d.status_line().contains("Signed out"), "says why: {}", s.d.status_line());
    // Booted, with nothing failed: `r` says there is nothing to retry.
    let mut s = Scripted::booted(false, None);
    s.keys("r");
    assert!(s.b.held.is_empty());
    assert!(s.d.status_line().contains("Nothing to retry"), "{}", s.d.status_line());
}

#[test]
fn r_in_a_pane_whose_messages_failed_asks_for_them_again() {
    let mut s = opened("backend");
    s.b.fail(0, FailKind::Offline);
    s.pump();
    assert!(s.d.screen().contains("r retry"), "{}", s.d.screen());
    assert!(s.b.held.is_empty(), "nothing retries on its own");
    s.keys("r");
    assert_eq!(s.b.held.len(), 1, "asked again");
    assert!(matches!(s.b.held[0].command, Command::History { .. }));
    s.b.answer(0);
    assert!(s.pump());
    assert!(!s.d.screen().contains("Could not load messages"), "{}", s.d.screen());
}
