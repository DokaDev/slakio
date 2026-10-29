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
use slakio_core::backend::{BackendHandle, Capabilities, Connection, Envelope, FailKind, Generation, Live};
use slakio_core::i18n::{I18n, Label, Lang};
use slakio_core::model::WorkspaceId;
use slakio_tui::app::model::SignedIn;
use slakio_tui::app::{App, Effect, Settings};
use slakio_tui::demo::DemoBackend;
use slakio_tui::keymap::parse_keys;
use slakio_tui::theme::Theme;
use slakio_tui::{exchange, many};
use slakio_world::World;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
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
            SignedIn { id: ia, name: "A company".into(), read_only: false },
            SignedIn { id: ib, name: "B side".into(), read_only: false },
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
            let a = SignedIn { id: ids().0, name: "A company".into(), read_only: false };
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
