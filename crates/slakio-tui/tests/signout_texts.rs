//! What the app says when Slack does not accept a workspace's sign-in: signed out while it runs,
//! the notice keeps `:login` whole at 80, 100 and 120 columns (the workspace's name is cut
//! first), with one workspace and with two, in English and Korean, and saying it again (`gg`,
//! `:retry`) names `:login` too; refused at the start, the words name the command line's
//! `slakio login`; the sign-in screen over the app lists `Esc` to go back.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use slakio_core::backend::{
    BackendHandle, Capabilities, Connection, Envelope, Event as Answer, FailKind, Generation, Live,
};
use slakio_core::i18n::{I18n, Label, Lang};
use slakio_core::model::WorkspaceId;
use slakio_tui::app::model::SignedIn;
use slakio_tui::app::{App, Settings};
use slakio_tui::demo::DemoBackend;
use slakio_tui::keymap::parse_keys;
use slakio_tui::theme::Theme;
use slakio_tui::{exchange, many};
use slakio_world::World;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

fn ids() -> Vec<WorkspaceId> {
    World::demo().snapshot().workspaces.iter().map(|w| w.id.clone()).collect()
}

/// A read-only workspace backend over the demo world that, once `gone`, refuses every request
/// for the sign-in (as Slack's backend does once signed out).
fn revocable(gone: Arc<AtomicBool>) -> BackendHandle {
    let BackendHandle { tx: inner, rx: mut answers, .. } = exchange::serve(Box::new(DemoBackend::new(World::demo())));
    let (handle, mut end) = BackendHandle::pair(Capabilities { demo: false, read_only: true }, 16);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                r = end.rx.recv() => {
                    let Some(r) = r else { return };
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

/// The app at `w`×`h` over the first `n` workspaces of the demo world (A, then B), read-only
/// like Slack's backend (so the status line also says `read only`).
struct Run {
    d: Demo,
    handle: BackendHandle,
    /// A's (index 0) and B's: once set, Slack no longer accepts their sign-in.
    gone: [Arc<AtomicBool>; 2],
}

impl Run {
    fn new(n: usize, w: u16, h: u16, lang: Lang) -> Self {
        Self::refusing(n, w, h, lang, false)
    }

    /// [`Run::new`], B's sign-in refused from the start when `b_refused`.
    fn refusing(n: usize, w: u16, h: u16, lang: Lang, b_refused: bool) -> Self {
        let names = ["A company", "B side"];
        let gone: [Arc<AtomicBool>; 2] = [Arc::default(), Arc::new(AtomicBool::new(b_refused))];
        let backends = (ids().into_iter().zip(&gone)).take(n).map(|(id, g)| (id, revocable(Arc::clone(g)))).collect();
        let handle = many::serve(backends);
        let mut d = Demo::new(w, h);
        let mut app = App::new(lang, Theme::terminal());
        app.settings = Settings::default();
        app.resize(w, h);
        let signed: Vec<SignedIn> = (ids().into_iter().zip(names))
            .take(n)
            .map(|(id, name)| SignedIn { id, name: name.into(), read_only: true, user: String::new() })
            .collect();
        app.connect_workspaces(handle.capabilities, &signed);
        d.app = app;
        Self { d, handle, gone }
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
            self.d.app.handle_event(ratatui::crossterm::event::Event::Key(k.to_event()), self.d.now);
        }
        self.settle().await;
    }

    /// Long enough for the notice shown to go.
    fn later(&mut self) {
        self.d.now += Duration::from_secs(60);
        self.d.app.on_tick(self.d.now);
    }

    /// The live updates of A say Slack signed it out.
    fn sign_out_a(&mut self) {
        let state = Connection::SignedOut;
        let e = Envelope {
            generation: Generation::PUSHED,
            event: Answer::Live(Box::new(Live::Connection { workspace: ids()[0].clone(), state })),
        };
        self.d.app.on_backend(e, self.d.now);
    }
}

/// The sign-out notice of A, whole but for the workspace's name (cut, never under eight cells).
fn notice_whole(status: &str, i18n: &I18n, several: bool) -> bool {
    if !several {
        return status.contains(&i18n.label(Label::SignedOut).to_string());
    }
    let why = i18n.label(Label::SignedOutWhy).to_string();
    ["A company", "A compa…"].iter().any(|name| status.contains(&format!("{name}: {why}")))
}

#[test]
fn the_sign_out_notice_keeps_login_whole_at_every_width() {
    rt().block_on(async {
        for n in [1, 2] {
            for lang in [Lang::En, Lang::Ko] {
                for w in [80u16, 100, 120] {
                    for place in ["deploys", "design-review"] {
                        let at = format!("{n} workspace(s) {lang:?} {w} #{place}");
                        let mut r = Run::new(n, w, 24, lang);
                        r.settle().await;
                        r.d.list_cursor_on(place);
                        r.keys("enter").await;
                        r.sign_out_a();
                        let status = demo::mask_hangul(&r.d.status_line());
                        let i18n = I18n::new(lang);
                        let masked = |s: String| demo::mask_hangul(&s);
                        let whole = notice_whole(&r.d.status_line(), &i18n, n > 1);
                        assert!(whole, "{at}: the notice whole: |{status}|");
                        let mark = masked(i18n.label(Label::StatusSignedOut).to_string());
                        assert!(status.contains(&mark), "{at}: the mark: |{status}|");
                        assert!(status.contains(&masked(i18n.label(Label::StatusReadOnly).to_string())), "{at}");
                    }
                }
            }
        }
    });
}

#[test]
fn saying_it_again_after_a_sign_out_while_running_names_login() {
    rt().block_on(async {
        for (n, lang, w) in [(2, Lang::En, 80), (2, Lang::Ko, 120), (1, Lang::En, 100), (1, Lang::Ko, 80)] {
            let at = format!("{n} workspace(s) {lang:?} {w}");
            let i18n = I18n::new(lang);
            let cli = i18n.label(Label::FailAuth).to_string();
            let mut r = Run::new(n, w, 24, lang);
            r.settle().await;
            r.d.list_cursor_on("big-history");
            r.keys("enter").await;
            // Slack signs A out; the next page asked for is refused.
            r.gone[0].store(true, Ordering::SeqCst);
            r.keys("g g").await;
            assert!(notice_whole(&r.d.status_line(), &i18n, n > 1), "{at}: |{}|", r.d.status_line());
            for again in ["g g", "r", ": r e t r y enter"] {
                r.later();
                r.keys(again).await;
                let status = r.d.status_line();
                assert!(!status.contains(&cli), "{at} {again}: no `slakio login`: |{status}|");
                if again != "g g" {
                    assert!(notice_whole(&status, &i18n, n > 1), "{at} {again}: |{status}|");
                }
            }
        }
    });
}

#[test]
fn refused_at_the_start_the_words_name_slakio_login() {
    rt().block_on(async {
        for (lang, w) in [(Lang::En, 120), (Lang::En, 80), (Lang::Ko, 100)] {
            let at = format!("{lang:?} {w}");
            let i18n = I18n::new(lang);
            let mut r = Run::refusing(2, w, 24, lang, true);
            r.settle().await;
            r.later();
            // Shown, B's list says to run `slakio login`, and so does asking again.
            r.keys("space W j enter").await;
            assert_eq!(r.d.app.failed_why(), Some(Label::FailAuth), "{at}: {}", r.d.screen());
            assert!(r.d.screen().contains("slakio"), "{at}: {}", r.d.screen());
            r.keys("r").await;
            assert!(!r.d.status_line().contains(&i18n.label(Label::SignedOutWhy).to_string()), "{at}");
            assert!(r.d.app.boot_failed_in(&ids()[1]) == Some(&FailKind::Auth), "{at}");
            assert!(!r.d.app.signed_out_in(&ids()[1]), "{at}: not signed out while running");
        }
    });
}

#[test]
fn the_sign_in_screen_over_the_app_lists_esc_to_go_back() {
    rt().block_on(async {
        for lang in [Lang::En, Lang::Ko] {
            let back = I18n::new(lang).label(Label::SigninBack).to_string();
            let mut r = Run::new(2, 100, 30, lang);
            r.settle().await;
            r.d.app.offer_sign_in(None);
            let shown = r.d.screen();
            let line = shown.lines().find(|l| l.contains(&back)).unwrap_or_else(|| panic!("{lang:?}: {shown}"));
            assert!(line.contains("Esc"), "{lang:?}: {line}");
            r.keys("esc").await;
            assert!(r.d.app.sign_in().is_none(), "{lang:?}");
            // At the first sign-in there is nothing to go back to.
            let mut d = Demo::new(100, 30);
            d.app = App::new(lang, Theme::terminal());
            d.app.resize(100, 30);
            d.app.offer_sign_in(None);
            assert!(!d.screen().contains(&back), "{lang:?}: {}", d.screen());
        }
    });
}
