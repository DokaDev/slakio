//! `slakio` binary: parses the command line, owns the terminal and runs the event loop.

mod cli;
mod inapp;
mod signin;
mod stats;
mod store;
mod term;
mod words;

use cli::{Cli, HELP, parse_args};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::EventStream;
use slakio_auth::{Account, account};
use slakio_core::backend::{Backend, BackendHandle};
use slakio_core::config;
use slakio_core::fault::ErrorLog;
use slakio_core::i18n::{self, Label, Msg};
use slakio_core::paths::Paths;
use slakio_core::secret::{Credentials, MemoryStore, STORE_ENV, SecretStore, StoreKind};
use slakio_slack::governor::{Governor, Limits, SystemClock};
use slakio_slack::login;
use slakio_slack::request::Endpoints;
use slakio_tui::app::model::SignedIn as SignedInTo;
use slakio_tui::app::signin::Pasted;
use slakio_tui::app::status::Level;
use slakio_tui::app::{App, Effect, Settings};
use slakio_tui::demo::DemoBackend;
use slakio_tui::exchange;
use slakio_tui::terminal::{Cursor, cursor_shape, osc52};
use slakio_tui::theme::{Background, Look};
use slakio_tui::ui;
use slakio_world::World;
use std::io::{self, IsTerminal, Stdout, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

#[expect(clippy::too_many_lines, reason = "the binary's wiring; to be split")]
fn main() -> ExitCode {
    let mut stats = stats::Stats::from_env();
    // `login`: sign in first; `logout`: sign out and end.
    let (cli_config, demo, offline, login, logout, paste) = match parse_args(std::env::args_os().skip(1)) {
        Ok(Cli::Run { config, demo, offline }) => (config, demo, offline, false, None, false),
        Ok(Cli::Login { config, offline, paste }) => (config, false, offline, true, None, paste),
        Ok(Cli::Logout(which)) => (None, false, false, false, Some(which), false),
        Ok(Cli::Help) => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Ok(Cli::Version) => {
            println!("slakio {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    // Before anything else: which credential store this process may touch. A value that would
    // not be understood stops the program here, never later.
    let store_kind = match StoreKind::from_env(std::env::var(STORE_ENV).ok().as_deref()) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let store: Arc<dyn SecretStore> = match store_kind {
        StoreKind::Keychain => Arc::new(slakio_auth::keychain::Keychain),
        StoreKind::Memory => Arc::new(MemoryStore::new()),
    };
    // The desktop-app import touches the real Slack profile and keychain, so it is attempted only
    // under the real keychain store (never a `memory` development or test run), and never with
    // `--paste`.
    let desktop_ok = store_kind == StoreKind::Keychain && !paste;
    let paths = Paths::from_env();
    let config_path = cli_config.or_else(|| paths.config_file());
    let (cfg, cfg_err) = config::load(config_path.as_deref());
    // Whether anything may be sent at all: a value of the switch that is not understood stops
    // the program here, as the store's does.
    let network = match slakio_slack::governor::network_on(
        std::env::var(slakio_slack::governor::NETWORK_ENV).ok().as_deref(),
        &cfg.network,
    ) {
        Ok(on) => on && !offline,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let lang = i18n::detect_lang(&cfg.language, |k| std::env::var(k).ok());
    let workspaces = paths.workspaces_file();
    if let Some(which) = logout {
        return signin::sign_out(&*store, workspaces.as_deref(), &i18n::I18n::new(lang), &paths, &which);
    }
    // One governor for the whole process: the sign-in check counts toward its cap too.
    let governor = Governor::new(Limits::default(), Box::new(SystemClock::default()), network);
    signin::test_credentials(&*store, store_kind);
    let local = store::Local::new(&paths, &cfg);
    let mut wiring = Wiring {
        store,
        workspaces,
        governor,
        errors: paths.errors_log(),
        test_addrs: test_addrs(),
        desktop: None,
        local,
        live: cfg.live != "off",
        desktop_ok,
        replace: Arc::default(),
    };
    let env = |k: &str| std::env::var(k).ok();
    // The background matters only to a theme with a light and a dark variant, and to the
    // muted text of the terminal's colors; never asked without colors.
    let background =
        if env("NO_COLOR").is_some_and(|v| !v.is_empty()) { Background::Unknown } else { term::background() };
    let look = Look::from_env(env, background);
    let mut app = App::new(lang, look.theme(&cfg.theme));
    app.model.net.locked = !network;
    app.look = look;
    app.theme_setting.clone_from(&cfg.theme);
    app.settings = Settings {
        icons: cfg.icons == "on",
        // `image` (photos) comes later; until then it draws initials.
        avatars: cfg.avatars != "off",
        compact: cfg.density == "compact",
    };
    app.fold_views(cfg.nav_rows == "collapsed");
    // A sign-in piped in (`pbpaste | slakio login`): checked, kept, said, and the program ends.
    if login && !io::stdin().is_terminal() {
        return signin::sign_in_piped(&wiring, &i18n::I18n::new(lang));
    }
    // The one place that names a concrete backend: the demo, else the signed-in workspaces;
    // without one the app offers to sign in (also asked for with `slakio login`).
    let (kind, sign_in) = if demo {
        (Some(BackendKind::Demo(Box::new(DemoBackend::new(World::demo())))), None)
    } else if login {
        (None, Some(None))
    } else {
        match wiring.signed_in() {
            Ok(kept) if !kept.is_empty() => (Some(BackendKind::Slack(kept)), None),
            Ok(_) => (None, Some(None)),
            Err(fault) => {
                ErrorLog::new(paths.errors_log()).record("signin", &fault);
                (None, Some(Some(Label::SigninStoreRead)))
            }
        }
    };
    if let Some(why) = sign_in {
        // `slakio login` while signed in: what it signs in to is added beside those.
        let kept = wiring.workspaces.as_deref().and_then(|p| account::load(p).ok()).unwrap_or_default();
        app.sign_in_beside(kept.iter().map(|a| words::shown(&a.name)).collect());
        wiring.desktop = signin::offer_sign_in_screen(&mut app, desktop_ok, why).map(Arc::new);
    }
    if kind.is_some() {
        // Asked once, where the view switcher can preview the answer; the answer is saved.
        if cfg.icons == "ask" && config_path.is_some() && cfg_err.is_none() {
            app.ask_icons();
        }
    }
    // A config file that could not be used is never written over (a setting changed in the app
    // is then not saved, and the app says so).
    let writable = cfg_err.is_none();
    for key in &cfg.retired {
        app.warn(slakio_core::i18n::Msg::ConfigRetired { key: key.clone() }, Instant::now());
    }
    if let Some(e) = cfg_err {
        ErrorLog::new(paths.errors_log()).record("config", &e.fault());
        app.warn(words::config_message(&e), Instant::now());
    }
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    // Every thread's panic comes here (the hook is process-wide): the terminal is restored
    // before the message is printed.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        term::restore_terminal();
        default_hook(info);
    }));
    let result = rt.block_on(async {
        // Restores the terminal however this block ends (also a setup that fails part way).
        let _restore = term::guard();
        let (mut terminal, enhanced) = term::setup_terminal()?;
        app.keymap = slakio_tui::keymap::Keymap::new(enhanced);
        let backend = kind.map(|k| match k {
            BackendKind::Demo(b) => {
                app.connect(b.capabilities());
                exchange::serve(b)
            }
            BackendKind::Slack(kept) => {
                let mut notices = Vec::new();
                let (handle, signed) = wiring.connect(kept, &mut notices);
                app.connect_workspaces(handle.capabilities, &signed);
                // Queued: a boot that fails at once (offline) does not hide them.
                for n in notices {
                    app.status.queue(n, Level::Warning, Instant::now());
                }
                handle
            }
        });
        let saved = Saved { config: config_path.filter(|_| writable), errors: paths.errors_log() };
        run(&mut terminal, app, backend, &mut stats, saved, &mut wiring).await
    });
    // What the cache was asked to keep is written before the process ends.
    wiring.local.flush();
    if let Some(s) = stats.as_mut() {
        s.write();
    }
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Which backend the app runs.
enum BackendKind {
    /// The invented workspace (`--demo`): nothing connects anywhere.
    Demo(Box<DemoBackend>),
    /// Slack, read-only, for every signed-in workspace (in the workspaces file's order).
    Slack(Vec<Kept>),
}

/// What signing in and the Slack backend need: the secret store, the workspaces file, the
/// process's governor, the error log; in debug builds only, a fake Slack on this machine to
/// sign in to instead (the tests').
#[derive(Clone)]
struct Wiring {
    store: Arc<dyn SecretStore>,
    workspaces: Option<PathBuf>,
    governor: Arc<Governor>,
    errors: Option<PathBuf>,
    test_addrs: Vec<(Option<String>, std::net::SocketAddr)>,
    /// What the sign-in screen's desktop scan found (no secret but the tokens; the cookie is
    /// read only when a workspace is picked).
    desktop: Option<Arc<slakio_auth::Found>>,
    /// The local cache, put in front of the Slack backend.
    local: store::Local,
    /// Live updates (`live = "off"`: no socket opens; requests go on).
    live: bool,
    /// The Slack desktop app's sign-in may be read (the real keychain, no `--paste`).
    desktop_ok: bool,
    /// What replaces one workspace's backend in the handle the app runs with (signed in again).
    replace: Arc<std::sync::Mutex<Option<slakio_tui::many::Replace>>>,
}

/// `SLAKIO_TEST_SLACK_ADDR`: the loopback addresses the tests' fake Slacks listen on, used
/// instead of the workspaces' hosts: one address for every host, or `<host>=<address>` pairs
/// separated by commas (a host not named then reaches nothing). Debug builds only; a release
/// build never reads it.
fn test_addrs() -> Vec<(Option<String>, std::net::SocketAddr)> {
    if !cfg!(debug_assertions) {
        return Vec::new();
    }
    let Ok(value) = std::env::var("SLAKIO_TEST_SLACK_ADDR") else { return Vec::new() };
    let loopback = |a: &str| a.parse().ok().filter(|a: &std::net::SocketAddr| a.ip().is_loopback());
    value
        .split(',')
        .filter_map(|part| match part.split_once('=') {
            Some((host, addr)) => Some((Some(host.to_string()), loopback(addr)?)),
            None => Some((None, loopback(part)?)),
        })
        .collect()
}

/// A signed-in workspace: as the workspaces file names it, its credentials, and how to reach it.
struct Kept {
    account: Account,
    credentials: Credentials,
    endpoints: Endpoints,
}

impl Wiring {
    fn endpoints(&self, host: &str) -> Option<Endpoints> {
        if self.test_addrs.is_empty() {
            return Endpoints::workspace(host);
        }
        let to = self.test_addrs.iter().find(|(h, _)| h.as_deref().is_none_or(|h| h == host))?;
        Endpoints::local(to.1)
    }

    /// Every signed-in workspace whose credentials the store has, in the file's order. One
    /// whose credentials cannot be read is left out (the error log says why) and the others are
    /// used; only when none can be had is that a fault.
    fn signed_in(&self) -> Result<Vec<Kept>, slakio_core::fault::Fault> {
        let Some(path) = &self.workspaces else { return Ok(Vec::new()) };
        let (mut kept, mut fault) = (Vec::new(), None);
        for account in account::load(path)? {
            let Some(endpoints) = self.endpoints(&account.host) else { continue };
            match account::credentials(&*self.store, &account) {
                Ok(Some(credentials)) => kept.push(Kept { account, credentials, endpoints }),
                Ok(None) => self.record(&slakio_core::fault::Fault::other(format!(
                    "workspace {}: no credentials in the keychain; left out (slakio login signs in again)",
                    account.team
                ))),
                Err(f) => {
                    self.record(&f);
                    fault = Some(f);
                }
            }
        }
        match fault {
            Some(f) if kept.is_empty() => Err(f),
            _ => Ok(kept),
        }
    }

    /// [`Wiring::signed_in`], then [`Wiring::connect`], off the loop's thread (the keychain may
    /// ask; claiming each workspace's part of the cache may wait).
    async fn connect_all(&self) -> Result<(BackendHandle, Vec<SignedInTo>, Vec<Msg>), Label> {
        let w = self.clone();
        let connected = tokio::task::spawn_blocking(move || {
            let kept = w.signed_in().ok().filter(|k| !k.is_empty())?;
            let mut notices = Vec::new();
            let (handle, signed) = w.connect(kept, &mut notices);
            Some((handle, signed, notices))
        });
        connected.await.ok().flatten().ok_or(Label::SigninStoreRead)
    }

    /// A workspace's backend, with its live updates unless they are off.
    fn serve(&self, credentials: Credentials, endpoints: Endpoints) -> BackendHandle {
        let client = slakio_slack::Client::new(credentials, endpoints, Arc::clone(&self.governor));
        slakio_slack::serve_with(client, self.live.then(slakio_slack::Realtime::default))
    }

    /// One backend per workspace of `kept`, each with the cache in front of it (what to tell
    /// about the cache goes to `notices`), behind one handle; and the workspaces as the app
    /// lists them.
    fn connect(&self, kept: Vec<Kept>, notices: &mut Vec<Msg>) -> (BackendHandle, Vec<SignedInTo>) {
        let mut handles = Vec::with_capacity(kept.len());
        let mut signed = Vec::with_capacity(kept.len());
        for k in kept {
            let (team, user) = (&k.account.team, &k.account.user);
            let handle = self.local.front(self.serve(k.credentials, k.endpoints), team, user, notices);
            let id = slakio_core::model::WorkspaceId::new(team);
            let read_only = handle.capabilities.read_only;
            signed.push(SignedInTo { id: id.clone(), name: k.account.name.clone(), read_only });
            handles.push((id, handle));
        }
        let (handle, replace) = slakio_tui::many::serve_replaceable(handles);
        *self.replace.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(replace);
        (handle, signed)
    }

    /// Read `text`, ask Slack whether it signs in (one call), and keep it; or the words for why
    /// not. Nothing is kept unless Slack accepted it.
    async fn sign_in(self, text: Pasted) -> Result<Kept, Label> {
        let pasted = slakio_auth::parse(&text.0).map_err(words::paste_label)?;
        drop(text);
        self.finish(pasted.credentials, pasted.host).await
    }

    /// Sign in with the picked desktop workspace `team` (matched by team id in what the sign-in
    /// screen's scan found, never by position): read its credentials (the macOS keychain may
    /// prompt, off the loop's thread), then check and keep them.
    async fn sign_in_desktop(self, team: String) -> Result<Kept, Label> {
        let found = self.desktop.clone().ok_or(Label::SigninDesktopFailed)?;
        let ws = found.workspaces().iter().find(|w| w.team == team).ok_or(Label::SigninDesktopFailed)?;
        let host = ws.host.clone();
        // Signed in from this same desktop session before: what was kept is used, without the
        // app's cookie (so without the Safe Storage prompt). Only when Slack no longer accepts
        // it is the cookie read.
        if let Some(kept) = found.reuse(&team, self.kept(&team).await) {
            match self.clone().finish(kept, host.clone()).await {
                Err(Label::SigninRefused) => {}
                done => return done,
            }
        }
        let creds = tokio::task::spawn_blocking(move || found.system_credentials(&team))
            .await
            .map_err(|_| Label::SigninDesktopFailed)?
            .map_err(words::desktop_label)?;
        self.finish(creds, host).await
    }

    /// The credentials kept for `team` by an earlier sign-in, when the workspaces file names it
    /// (the keychain may ask, off the loop's thread).
    async fn kept(&self, team: &str) -> Option<Credentials> {
        let (path, store, team) = (self.workspaces.clone()?, Arc::clone(&self.store), team.to_string());
        tokio::task::spawn_blocking(move || {
            let a = account::load(&path).ok()?.into_iter().find(|a| a.team == team)?;
            account::credentials(&*store, &a).ok().flatten()
        })
        .await
        .ok()
        .flatten()
    }

    /// Check `credentials` for the workspace at `host` with one `client.userBoot` and keep them
    /// if Slack accepts; the words for why not otherwise. Nothing is kept unless Slack accepted.
    async fn finish(self, credentials: Credentials, host: String) -> Result<Kept, Label> {
        let endpoints = self.endpoints(&host).ok_or(Label::SigninNotWorkspace)?;
        let client = slakio_slack::Client::new(credentials.clone(), endpoints.clone(), Arc::clone(&self.governor));
        let who = login::check(&client).await.map_err(|e| {
            self.record(&slakio_core::fault::Fault::other(e.detail()));
            words::check_label(&e.kind())
        })?;
        let account =
            Account { name: who.team_name.unwrap_or_else(|| who.team.clone()), team: who.team, host, user: who.user };
        let path = self.workspaces.clone().ok_or(Label::SigninStoreFailed)?;
        // The keychain can ask the user (a window of macOS): off the loop's thread.
        let (store, kept) = (Arc::clone(&self.store), account.clone());
        let creds = credentials.clone();
        let stored = tokio::task::spawn_blocking(move || account::keep(&*store, &path, &kept, &creds)).await;
        match stored {
            Ok(Ok(())) => Ok(Kept { account, credentials, endpoints }),
            Ok(Err(fault)) => {
                self.record(&fault);
                Err(Label::SigninStoreFailed)
            }
            Err(_) => Err(Label::SigninStoreFailed),
        }
    }

    fn record(&self, fault: &slakio_core::fault::Fault) {
        ErrorLog::new(self.errors.clone()).record("signin", fault);
    }
}

/// The event loop: draw when something changed, then sleep until the next input event, signal,
/// backend event or deadline of the app. With nothing to do it never wakes up.
/// Where the loop saves what the app asks it to: the config file, and the error log for a save
/// that fails.
struct Saved {
    config: Option<std::path::PathBuf>,
    errors: Option<std::path::PathBuf>,
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mut app: App,
    mut backend: Option<BackendHandle>,
    stats: &mut Option<stats::Stats>,
    saved: Saved,
    wiring: &mut Wiring,
) -> io::Result<()> {
    let size = terminal.size()?;
    app.resize(size.width, size.height);
    let mut events = EventStream::new();
    let mut signals = term::Signals::new()?;
    let mut cursor = Cursor::default();
    let mut redraw = true;
    // A paste being checked with Slack.
    let mut signing_in: Option<tokio::task::JoinHandle<Result<inapp::Done, Label>>> = None;
    // Debug builds only: panic after the first frame, for the test that the panic hook
    // restores the terminal (tests/signals.rs).
    let panic_after_frame = cfg!(debug_assertions) && std::env::var_os("SLAKIO_DEBUG_PANIC").is_some_and(|v| v == "1");
    while !app.quit {
        // The backend's turn: deliver what the app asked for, take what is ready.
        if let Some(b) = backend.as_mut() {
            redraw |= exchange::pump(&mut app, b).await;
        }
        for effect in app.take_effects() {
            match effect {
                Effect::Copy(text) => {
                    let mut out = io::stdout();
                    out.write_all(osc52(&text).as_bytes())?;
                    out.flush()?;
                }
                Effect::Record { context, fault } => ErrorLog::new(saved.errors.clone()).record(context, &fault),
                Effect::SignIn(pasted) => signing_in = Some(tokio::spawn(wiring.clone().sign_in_app(pasted))),
                Effect::DesktopPick(t) => signing_in = Some(tokio::spawn(wiring.clone().sign_in_desktop_all(t))),
                Effect::Network(on) => app.net_switched(on, wiring.governor.switch(on), Instant::now()),
                Effect::SignInAgain(team) => inapp::offer(&mut app, wiring, team.as_deref()),
                Effect::Save { key, value } => {
                    let result = match &saved.config {
                        Some(path) => config::set(path, key, &value),
                        None => Err(slakio_core::fault::Fault::other("no config directory")),
                    };
                    if let Err(fault) = result {
                        ErrorLog::new(saved.errors.clone()).record("config", &fault);
                        app.warn(Msg::Label(Label::ConfigSaveFailed), Instant::now());
                        redraw = true;
                    }
                }
            }
        }
        if redraw {
            cursor.apply(&mut io::stdout(), cursor_shape(&app))?;
            terminal.draw(|f| ui::draw(f, &app, Instant::now()))?;
            if let Some(s) = stats.as_mut() {
                s.drawn();
            }
            if panic_after_frame {
                panic!("SLAKIO_DEBUG_PANIC: a panic after the first frame");
            }
        }
        if let Some(s) = stats.as_mut() {
            s.woke();
        }
        let deadline = app.deadline();
        let timer = async {
            match deadline {
                Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
                None => std::future::pending().await,
            }
        };
        redraw = tokio::select! {
            ev = events.next() => match ev {
                Some(Ok(ev)) => app.handle_event(ev, Instant::now()),
                Some(Err(e)) => return Err(e),
                // The input stream ended: nothing can ever ask to quit, so quit now.
                None => break,
            },
            _ = timer => app.on_tick(Instant::now()),
            // An answer that came later than its turn: it wakes the loop like a key.
            envelope = exchange::next(backend.as_mut()) => app.on_backend(envelope, Instant::now()),
            done = async { signing_in.as_mut().expect("guarded").await }, if signing_in.is_some() => {
                signing_in = None;
                inapp::signed_in(done.unwrap_or(Err(Label::SigninFailed)), &mut app, &mut backend);
                true
            }
            // Asked to end from outside: the loop ends and the terminal is restored on the way
            // out, as on a normal quit.
            _ = signals.recv() => {
                app.quit = true;
                false
            }
        };
    }
    Ok(())
}
