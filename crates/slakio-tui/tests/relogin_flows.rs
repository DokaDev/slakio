//! A workspace Slack signs out while the app runs, and signing in to it again from the app: it
//! stays on screen as it was, marked `signed out, :login`, and asks nothing again by itself;
//! `:login` asks the binary for the sign-in screen with that workspace's team id, the screen
//! shows over the app and `Esc` goes back; signed in again, only its backend is put in place of
//! the old one (`many::Replace`): it boots again and its pages come back, while its tabs, panes
//! and drafts stay and the other workspace is asked nothing. A sign-out said by the live
//! updates is said the same way. At 120x40 and 80x24, in English and Korean.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use slakio_core::backend::{
    BackendHandle, Capabilities, Command, Connection, Envelope, Event as Answer, FailKind, Generation, Live, Request,
};
use slakio_core::i18n::{I18n, Label, Lang};
use slakio_core::model::{Target, WorkspaceId};
use slakio_tui::action::{Action, AppAction};
use slakio_tui::app::model::SignedIn;
use slakio_tui::app::signin::SignIn;
use slakio_tui::app::{App, Effect, Settings};
use slakio_tui::demo::DemoBackend;
use slakio_tui::keymap::parse_keys;
use slakio_tui::theme::Theme;
use slakio_tui::{exchange, many};
use slakio_world::World;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

/// The user signed in to A (the same user signs in again).
fn me_a() -> String {
    World::demo().snapshot().workspaces[0].me.as_str().to_string()
}

fn ids() -> (WorkspaceId, WorkspaceId) {
    let w = World::demo();
    let s = w.snapshot();
    (s.workspaces[0].id.clone(), s.workspaces[1].id.clone())
}

/// A workspace's backend over the demo world (changes allowed, so a draft can be written) that
/// counts the requests it is handed (`asked`) and, once `gone`, refuses each for the sign-in.
fn revocable(gone: Arc<AtomicBool>, asked: Arc<AtomicUsize>) -> BackendHandle {
    let BackendHandle { tx: inner, rx: mut answers, .. } = exchange::serve(Box::new(DemoBackend::new(World::demo())));
    let (handle, mut end) = BackendHandle::pair(Capabilities { demo: false, read_only: false }, 16);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                r = end.rx.recv() => {
                    let Some(r) = r else { return };
                    asked.fetch_add(1, Ordering::SeqCst);
                    let sent = if gone.load(Ordering::SeqCst) {
                        end.tx.send(Envelope::failed(r.id, FailKind::Auth, "invalid_auth")).await.is_ok()
                    } else {
                        inner.send(r).await.is_ok()
                    };
                    if !sent {
                        return;
                    }
                }
                e = answers.recv() => {
                    let Some(e) = e else { return };
                    if end.tx.send(e).await.is_err() {
                        return;
                    }
                }
            }
        }
    });
    handle
}

/// The app over A (the one Slack signs out) and B, behind one handle whose backends can be
/// replaced.
struct Two {
    d: Demo,
    handle: BackendHandle,
    replace: many::Replace,
    gone: Arc<AtomicBool>,
    asked_a: Arc<AtomicUsize>,
    asked_b: Arc<AtomicUsize>,
}

impl Two {
    fn new(w: u16, h: u16, lang: Lang) -> Self {
        let (ia, ib) = ids();
        let (gone, asked_a, asked_b) = (Arc::default(), Arc::default(), Arc::default());
        let a = revocable(Arc::clone(&gone), Arc::clone(&asked_a));
        let b = revocable(Arc::default(), Arc::clone(&asked_b));
        let (handle, replace) = many::serve_replaceable(vec![(ia.clone(), a), (ib.clone(), b)]);
        let mut d = Demo::new(w, h);
        let mut app = App::new(lang, Theme::terminal());
        app.settings = Settings::default();
        app.resize(w, h);
        let signed = [
            SignedIn { id: ia, name: "A company".into(), read_only: false, user: String::new() },
            SignedIn { id: ib, name: "B side".into(), read_only: false, user: String::new() },
        ];
        app.connect_workspaces(handle.capabilities, &signed);
        d.app = app;
        Self { d, handle, replace, gone, asked_a, asked_b }
    }

    async fn settle(&mut self) {
        exchange::pump(&mut self.d.app, &mut self.handle).await;
        while let Ok(Some(e)) = tokio::time::timeout(Duration::from_millis(150), self.handle.rx.recv()).await {
            self.d.app.on_backend(e, self.d.now);
            exchange::pump(&mut self.d.app, &mut self.handle).await;
        }
    }

    async fn keys(&mut self, notation: &str) {
        for k in parse_keys(notation).unwrap() {
            self.d.app.handle_event(Event::Key(k.to_event()), self.d.now);
        }
        self.settle().await;
    }

    async fn text(&mut self, text: &str) {
        for c in text.chars() {
            self.d.app.handle_event(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)), self.d.now);
        }
        self.settle().await;
    }

    fn label(&self, l: Label) -> String {
        self.d.app.i18n.label(l).to_string()
    }

    fn sign_ins_asked(&mut self) -> Vec<Option<String>> {
        let effects = self.d.app.take_effects();
        effects.into_iter().filter_map(|e| if let Effect::SignInAgain(t) = e { Some(t) } else { None }).collect()
    }
}

#[test]
fn a_workspace_signed_out_while_running_keeps_its_panes_and_drafts_and_alone_signs_in_again() {
    rt().block_on(async {
        for (lang, w, h) in [(Lang::En, 120, 40), (Lang::En, 80, 24), (Lang::Ko, 120, 40), (Lang::Ko, 80, 24)] {
            let at = format!("{lang:?} {w}x{h}");
            let mut t = Two::new(w, h, lang);
            t.settle().await;
            // A conversation of A open, a draft in it; another in a tab of its own.
            t.d.list_cursor_on("general");
            t.keys("enter i").await;
            t.text("half a thought").await;
            t.keys("esc").await;
            assert!(t.d.screen().contains("half a thought"), "{at}: {}", t.d.screen());
            // Slack signs A out: the next request is refused for the sign-in.
            t.gone.store(true, Ordering::SeqCst);
            t.d.list_cursor_on("backend");
            t.keys("t").await;
            let status = t.d.status_line();
            let mark = t.label(Label::StatusSignedOut);
            assert!(status.contains(&mark), "{at}: the mark: {status}");
            assert!(status.contains(&format!("A company: {}", t.label(Label::SignedOutWhy))), "{at}: {status}");
            // Nothing more is asked of A by itself, however long; B is asked nothing new.
            let (a, b) = (t.asked_a.load(Ordering::SeqCst), t.asked_b.load(Ordering::SeqCst));
            for _ in 0..12 {
                t.d.now += Duration::from_secs(10);
                t.d.app.on_tick(t.d.now);
                t.settle().await;
            }
            assert_eq!(t.asked_a.load(Ordering::SeqCst), a, "{at}: no retry storm");
            // Long after the notice, the mark stays; the switcher says it too.
            let status = t.d.status_line();
            assert!(status.contains(&mark) && !status.contains(&t.label(Label::StatusLiveOff)), "{at}: {status}");
            t.keys("space W").await;
            let shown = t.d.screen();
            let marked =
                shown.lines().any(|l| l.contains("A company") && l.contains(&t.label(Label::SwitcherSignedOut)));
            assert!(marked, "{at}: the switcher's row: {shown}");
            t.keys("esc").await;
            // `:login` asks the binary for the sign-in screen, A's team id to put the cursor on.
            t.keys(":").await;
            t.text("login").await;
            t.keys("enter").await;
            assert_eq!(t.sign_ins_asked(), [Some(ids().0.as_str().to_string())], "{at}");
            // The screen shows over the app; Esc goes back to it as it was.
            t.d.app.offer_sign_in(None);
            let shown = t.d.screen();
            assert!(shown.contains(&t.label(Label::SigninWaiting)), "{at}: {shown}");
            let status = t.d.status_line();
            assert!(status.contains(&format!("Esc {}", t.label(Label::HintBack))), "{at}: {status}");
            t.keys("esc").await;
            assert!(t.d.app.sign_in().is_none() && t.d.screen().contains("#backend"), "{at}: {}", t.d.screen());
            // Signed in again: A's backend alone is put in place of the old one.
            t.d.app.offer_sign_in(None);
            let fresh = revocable(Arc::default(), Arc::clone(&t.asked_a));
            assert!(t.replace.put(&ids().0, fresh));
            let a = SignedIn { id: ids().0, name: "A company".into(), read_only: false, user: me_a() };
            t.d.app.signed_in_again(&[a], &["A company".to_string()], t.d.now);
            let before = t.asked_a.load(Ordering::SeqCst);
            t.settle().await;
            assert!(t.asked_a.load(Ordering::SeqCst) > before, "{at}: A booted again");
            assert!(t.d.app.sign_in().is_none(), "{at}");
            let status = t.d.status_line();
            assert!(!status.contains(&mark), "{at}: the mark goes: {status}");
            assert!(t.d.app.model.is_loaded_in(&ids().0), "{at}");
            // Its tab that failed has its messages now; the draft and the first tab are there.
            assert!(t.d.app.failed_here().is_none(), "{at}: {:?}", t.d.app.failed_here());
            t.keys("g T").await;
            assert!(t.d.screen().contains("half a thought"), "{at}: {}", t.d.screen());
            assert_eq!(t.asked_b.load(Ordering::SeqCst), b, "{at}: B was asked nothing");
        }
    });
}

#[test]
fn a_sign_out_the_live_updates_say_is_marked_and_said_once_at_every_width() {
    rt().block_on(async {
        for (lang, w, h) in [(Lang::En, 120, 40), (Lang::En, 80, 24), (Lang::Ko, 80, 24)] {
            let at = format!("{lang:?} {w}x{h}");
            let mut t = Two::new(w, h, lang);
            t.settle().await;
            let lost = |state| Envelope {
                generation: Generation::PUSHED,
                event: slakio_core::backend::Event::Live(Box::new(Live::Connection { workspace: ids().0, state })),
            };
            t.d.app.on_backend(lost(Connection::Connected { catch_up: false }), t.d.now);
            t.d.now += Duration::from_secs(30);
            t.d.app.on_tick(t.d.now);
            t.d.app.on_backend(lost(Connection::SignedOut), t.d.now);
            let status = t.d.status_line();
            let i18n = I18n::new(lang);
            let mark = i18n.label(Label::StatusSignedOut).to_string();
            assert!(status.contains(&mark), "{at}: {status}");
            assert!(status.contains(&i18n.label(Label::SignedOutWhy).to_string()), "{at}: the notice whole: {status}");
            assert!(!status.contains(&i18n.label(Label::StatusLiveOff).to_string()), "{at}: {status}");
            // Said again by a request refused later: not said twice.
            t.d.now += Duration::from_secs(30);
            t.d.app.on_tick(t.d.now);
            t.d.app.on_backend(lost(Connection::SignedOut), t.d.now);
            assert!(!t.d.status_line().contains(&i18n.label(Label::SignedOutWhy).to_string()), "{at}");
            assert!(t.d.status_line().contains(&mark), "{at}");
            if (lang, w) == (Lang::En, 80) {
                insta::assert_snapshot!("relogin_signed_out_80x24", t.d.snap());
            }
        }
    });
}

#[test]
fn the_demo_has_no_login() {
    let mut d = Demo::new(120, 40);
    d.keys(":");
    d.type_text("login");
    assert!(d.app.palette_rows().iter().all(|r| !r.name.contains(":login")), "{:?}", d.app.palette_rows());
    d.keys("esc");
    d.command("login");
    assert!(!d.app.take_effects().iter().any(|e| matches!(e, Effect::SignInAgain(_))));
}

/// A workspace's backend over the demo world that logs every request it is handed (`log`) and
/// holds, unanswered, those `hold` says (their ids go to `held`); the others the demo answers.
fn holding(
    log: Arc<Mutex<Vec<Command>>>,
    held: Arc<Mutex<Vec<Generation>>>,
    hold: fn(&Command) -> bool,
) -> BackendHandle {
    let BackendHandle { tx: inner, rx: mut answers, .. } = exchange::serve(Box::new(DemoBackend::new(World::demo())));
    let (handle, mut end) = BackendHandle::pair(Capabilities { demo: false, read_only: false }, 16);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                r = end.rx.recv() => {
                    let Some(r) = r else { return };
                    log.lock().unwrap().push(r.command.clone());
                    if hold(&r.command) {
                        held.lock().unwrap().push(r.id);
                    } else if inner.send(r).await.is_err() {
                        return;
                    }
                }
                e = answers.recv() => {
                    let Some(e) = e else { return };
                    if end.tx.send(e).await.is_err() {
                        return;
                    }
                }
            }
        }
    });
    handle
}

/// A's conversation `name`.
fn target_a(name: &str) -> Target {
    let w = World::demo();
    let ws = ids().0;
    Target::Conversation { workspace: ws.clone(), conversation: w.find(ws.as_str(), name).unwrap().id.clone() }
}

/// The app over A, whose backend is `a`, and B (which answers everything).
fn two_with(a: BackendHandle, w: u16, h: u16) -> Two {
    let (ia, ib) = ids();
    let (asked_b, gone) = (Arc::default(), Arc::default());
    let b = revocable(Arc::default(), Arc::clone(&asked_b));
    let (handle, replace) = many::serve_replaceable(vec![(ia.clone(), a), (ib.clone(), b)]);
    let mut d = Demo::new(w, h);
    let mut app = App::new(Lang::En, Theme::terminal());
    app.settings = Settings::default();
    app.resize(w, h);
    let signed = [
        SignedIn { id: ia, name: "A company".into(), read_only: false, user: me_a() },
        SignedIn { id: ib, name: "B side".into(), read_only: false, user: String::new() },
    ];
    app.connect_workspaces(handle.capabilities, &signed);
    d.app = app;
    Two { d, handle, replace, gone, asked_a: Arc::default(), asked_b }
}

fn again_as(user: String) -> [SignedIn; 1] {
    [SignedIn { id: ids().0, name: "A company".into(), read_only: false, user }]
}

#[test]
fn requests_the_replaced_backend_had_not_answered_are_answered_once_as_failed_for_the_network() {
    rt().block_on(async {
        let (ia, ib) = ids();
        let caps = Capabilities { demo: false, read_only: false };
        let (old, mut old_end) = BackendHandle::pair(caps, 16);
        let (b, _b_end) = BackendHandle::pair(caps, 16);
        let (mut handle, replace) = many::serve_replaceable(vec![(ia.clone(), old), (ib, b)]);
        let history = Command::History { target: target_a("general"), before: None, limit: 28 };
        handle.send(Request { id: Generation(7), workspace: None, command: history }).unwrap();
        handle.send(Request { id: Generation(8), workspace: Some(ia.clone()), command: Command::Boot }).unwrap();
        for _ in 0..2 {
            old_end.rx.recv().await.unwrap();
        }
        // The boot is answered before the swap: handed up once, and not answered again.
        old_end.tx.send(Envelope::failed(Generation(8), FailKind::Api { code: "x".into() }, "")).await.unwrap();
        let first = tokio::time::timeout(Duration::from_secs(2), handle.rx.recv()).await.unwrap().unwrap();
        assert_eq!(first.generation, Generation(8));
        let (fresh, _fresh_end) = BackendHandle::pair(caps, 16);
        assert!(replace.put(&ia, fresh));
        // The old backend's late refusal is not handed up: the page gets one answer, retryable.
        let _ = old_end.tx.send(Envelope::failed(Generation(7), FailKind::Auth, "invalid_auth")).await;
        let mut got = Vec::new();
        while let Ok(Some(e)) = tokio::time::timeout(Duration::from_millis(300), handle.rx.recv()).await {
            got.push(e);
        }
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].generation, Generation(7));
        assert!(matches!(&got[0].event, Answer::Failed { kind: FailKind::Network, .. }), "{got:?}");
    });
}

#[test]
fn a_page_on_its_way_when_signing_in_again_loads_and_a_late_refusal_marks_nothing() {
    rt().block_on(async {
        let (log, held) = (Arc::default(), Arc::default());
        let a = holding(Arc::clone(&log), Arc::clone(&held), |c| matches!(c, Command::History { .. }));
        let mut t = two_with(a, 120, 40);
        t.settle().await;
        t.d.list_cursor_on("general");
        t.keys("enter").await;
        assert_eq!(held.lock().unwrap().len(), 1, "the page is on its way");
        // Signed in again (same user) while A's old backend still holds it.
        let fresh_log: Arc<Mutex<Vec<Command>>> = Arc::default();
        let fresh = holding(Arc::clone(&fresh_log), Arc::default(), |_| false);
        assert!(t.replace.put(&ids().0, fresh));
        t.d.app.signed_in_again(&again_as(me_a()), &["A company".to_string()], t.d.now);
        t.settle().await;
        assert!(t.d.app.failed_here().is_none(), "{:?}", t.d.app.failed_here());
        let asked_again = fresh_log
            .lock()
            .unwrap()
            .iter()
            .any(|c| matches!(c, Command::History { target, .. } if *target == target_a("general")));
        assert!(asked_again, "asked again of the new backend: {:?}", fresh_log.lock().unwrap());
        assert!(!t.d.screen().contains(&t.label(Label::PaneLoading)), "{}", t.d.screen());
        // A refusal of the old backend arriving late marks nothing.
        let old = held.lock().unwrap()[0];
        t.d.app.on_backend(Envelope::failed(old, FailKind::Auth, "invalid_auth"), t.d.now);
        assert!(!t.d.app.signed_out_in(&ids().0));
        assert!(!t.d.status_line().contains(&t.label(Label::StatusSignedOut)), "{}", t.d.status_line());
    });
}

#[test]
fn signed_in_again_an_open_conversation_reading_back_an_older_page_is_caught_up_too() {
    rt().block_on(async {
        let gone = Arc::new(AtomicBool::new(false));
        let a = revocable(Arc::clone(&gone), Arc::default());
        let mut t = two_with(a, 120, 40);
        t.settle().await;
        t.d.list_cursor_on("big-history");
        t.keys("enter").await;
        // Slack signs A out; reading back is refused for the sign-in.
        gone.store(true, Ordering::SeqCst);
        t.keys("g g").await;
        assert!(t.d.app.signed_out_in(&ids().0), "{}", t.d.screen());
        // Signed in again: the older page goes out again (held here), and the newest end of the
        // conversation is read again as well, so what was posted meanwhile shows.
        let fresh_log: Arc<Mutex<Vec<Command>>> = Arc::default();
        let fresh =
            holding(Arc::clone(&fresh_log), Arc::default(), |c| matches!(c, Command::History { before: Some(_), .. }));
        assert!(t.replace.put(&ids().0, fresh));
        // Older pages are paced: the one asked again goes out at once only past its gap.
        tokio::time::sleep(Duration::from_millis(1600)).await;
        t.d.app.signed_in_again(&again_as(me_a()), &["A company".to_string()], t.d.now);
        t.settle().await;
        let log = fresh_log.lock().unwrap().clone();
        assert!(log.iter().any(|c| matches!(c, Command::History { before: Some(_), .. })), "{log:?}");
        assert!(
            log.iter().any(|c| matches!(c, Command::Catchup { target, .. } if *target == target_a("big-history"))),
            "{log:?}"
        );
    });
}

#[test]
fn signed_in_again_as_someone_else_nothing_of_the_user_before_stays() {
    rt().block_on(async {
        let mut t = Two::new(120, 40, Lang::En);
        t.settle().await;
        t.d.list_cursor_on("general");
        t.keys("enter i").await;
        t.text("half a thought").await;
        t.keys("esc").await;
        assert!(t.d.screen().contains("Message #general"), "{}", t.d.screen());
        t.gone.store(true, Ordering::SeqCst);
        t.keys("g g").await;
        let fresh_log: Arc<Mutex<Vec<Command>>> = Arc::default();
        let fresh = holding(Arc::clone(&fresh_log), Arc::default(), |c| matches!(c, Command::Boot));
        assert!(t.replace.put(&ids().0, fresh));
        t.d.app.signed_in_again(&again_as("UOTHER".into()), &["A company".to_string()], t.d.now);
        // At once: A's lists, panes and draft are gone; B's are not touched.
        let a = ids().0;
        assert!(t.d.app.model.conversations().iter().all(|c| c.workspace != a));
        assert!(t.d.app.model.conversations().iter().any(|c| c.workspace == ids().1));
        assert!(t.d.app.model.me(&a).is_none());
        t.settle().await;
        let shown = t.d.screen();
        assert!(!shown.contains("half a thought") && !shown.contains("Message #general"), "{shown}");
        // Nothing of the user before is asked of the new sign-in: its boot only.
        assert_eq!(*fresh_log.lock().unwrap(), [Command::Boot]);
        // No draft is left to ask about.
        t.d.app.dispatch(Action::App(AppAction::Quit), t.d.now);
        assert!(t.d.app.quit, "nothing unsent is left");
    });
}

#[test]
fn while_a_sign_in_is_checked_no_other_can_start() {
    rt().block_on(async {
        let mut t = Two::new(120, 40, Lang::En);
        t.settle().await;
        t.d.app.offer_sign_in(None);
        t.d.app.handle_event(Event::Paste("curl 'https://a.slack.com/api/x'".into()), t.d.now);
        assert_eq!(t.d.app.sign_in(), Some(SignIn::Checking));
        let started = |t: &mut Two| t.d.app.take_effects().iter().filter(|e| matches!(e, Effect::SignIn(_))).count();
        assert_eq!(started(&mut t), 1);
        // Neither the command line nor the palette opens; `:login` cannot be asked.
        t.keys(":").await;
        t.text("login").await;
        t.keys("enter").await;
        t.keys("ctrl+p").await;
        assert!(!t.d.app.cmdline.is_open());
        assert!(t.sign_ins_asked().is_empty());
        assert_eq!(t.d.app.sign_in(), Some(SignIn::Checking), "still the one being checked");
        t.d.app.handle_event(Event::Paste("curl 'https://a.slack.com/api/y'".into()), t.d.now);
        assert_eq!(started(&mut t), 0, "one at a time");
    });
}

#[test]
fn a_change_refused_for_the_sign_in_marks_the_workspace_signed_out() {
    rt().block_on(async {
        let mut t = Two::new(120, 40, Lang::En);
        t.settle().await;
        t.keys("space o").await;
        t.gone.store(true, Ordering::SeqCst);
        t.keys("x").await;
        assert!(t.d.app.signed_out_in(&ids().0));
        assert!(t.d.status_line().contains(&t.label(Label::StatusSignedOut)), "{}", t.d.status_line());
    });
}

#[test]
fn a_workspace_refused_at_the_start_while_shown_from_the_cache_says_run_slakio_login() {
    rt().block_on(async {
        let (ia, ib) = ids();
        let caps = Capabilities { demo: false, read_only: false };
        // A's backend: the cache's boot first, then Slack refuses the sign-in.
        let (a, mut a_end) = BackendHandle::pair(caps, 16);
        let cached = World::demo().snapshot().only(&ia);
        tokio::spawn(async move {
            while let Some(r) = a_end.rx.recv().await {
                let cached = Envelope { generation: r.id, event: Answer::CachedBoot(Box::new(cached.clone())) };
                let _ = a_end.tx.send(cached).await;
                let _ = a_end.tx.send(Envelope::failed(r.id, FailKind::Auth, "invalid_auth")).await;
            }
        });
        let mut t = two_with(a, 120, 40);
        let _ = ib;
        t.settle().await;
        assert!(t.d.app.model.is_cached_in(&ia), "shown from the cache");
        assert!(!t.d.app.signed_out_in(&ia));
        let status = t.d.status_line();
        assert!(status.contains("A company: signed out, run slakio login"), "{status}");
        assert!(!status.contains(&t.label(Label::StatusSignedOut)), "{status}");
    });
}
