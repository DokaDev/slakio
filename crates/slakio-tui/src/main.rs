//! `slakio` binary: parses the command line, owns the terminal and runs the event loop.

mod cli;
mod stats;
mod term;

use cli::{Cli, HELP, parse_args};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::EventStream;
use slakio_auth::{Account, PasteError, account};
use slakio_core::backend::{Backend, BackendHandle};
use slakio_core::config::{self, ConfigError};
use slakio_core::fault::ErrorLog;
use slakio_core::i18n::{self, Label, Msg};
use slakio_core::paths::Paths;
use slakio_core::sanitize::Remote;
use slakio_core::secret::{Credentials, MemoryStore, STORE_ENV, SecretStore, StoreKind};
use slakio_slack::governor::{Governor, Limits, SystemClock};
use slakio_slack::request::Endpoints;
use slakio_slack::{SlackError, login};
use slakio_tui::app::signin::{Pasted, PickerRow};
use slakio_tui::app::{App, Effect, Settings};
use slakio_tui::demo::DemoBackend;
use slakio_tui::exchange;
use slakio_tui::terminal::{Cursor, cursor_shape, osc52};
use slakio_tui::theme::{Background, Look};
use slakio_tui::ui;
use slakio_world::World;
use std::io::{self, IsTerminal, Read, Stdout, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

#[expect(clippy::too_many_lines, reason = "the binary's wiring; to be split")]
fn main() -> ExitCode {
    let mut stats = stats::Stats::from_env();
    // `login`: sign in first; `logout`: sign out and end.
    let (cli_config, demo, offline, login, logout, paste) = match parse_args(std::env::args_os().skip(1)) {
        Ok(Cli::Run { config, demo, offline }) => (config, demo, offline, false, false, false),
        Ok(Cli::Login { config, offline, paste }) => (config, false, offline, true, false, paste),
        Ok(Cli::Logout) => (None, false, false, false, true, false),
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
    if logout {
        return sign_out(&*store, workspaces.as_deref(), &i18n::I18n::new(lang), paths.errors_log());
    }
    // One governor for the whole process: the sign-in check counts toward its cap too.
    let governor = Governor::new(Limits::default(), Box::new(SystemClock::default()), network);
    let mut wiring =
        Wiring { store, workspaces, governor, errors: paths.errors_log(), test_addr: test_addr(), desktop: None };
    let env = |k: &str| std::env::var(k).ok();
    // The background matters only to a theme with a light and a dark variant, and to the
    // muted text of the terminal's colors; never asked without colors.
    let background =
        if env("NO_COLOR").is_some_and(|v| !v.is_empty()) { Background::Unknown } else { term::background() };
    let look = Look::from_env(env, background);
    let mut app = App::new(lang, look.theme(&cfg.theme));
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
        return sign_in_piped(&wiring, &i18n::I18n::new(lang));
    }
    // The one place that names a concrete backend: the demo, else the signed-in workspace;
    // without one the app offers to sign in (also asked for with `slakio login`).
    let (kind, sign_in) = if demo {
        (Some(BackendKind::Demo(Box::new(DemoBackend::new(World::demo())))), None)
    } else if login {
        (None, Some(None))
    } else {
        match wiring.signed_in() {
            Ok(Some((credentials, endpoints))) => (Some(BackendKind::Slack { credentials, endpoints }), None),
            Ok(None) => (None, Some(None)),
            Err(fault) => {
                ErrorLog::new(paths.errors_log()).record("signin", &fault);
                (None, Some(Some(Label::SigninStoreRead)))
            }
        }
    };
    if let Some(why) = sign_in {
        wiring.desktop = offer_sign_in_screen(&mut app, desktop_ok, why).map(Arc::new);
    }
    let capabilities = kind.as_ref().map(|k| match k {
        BackendKind::Demo(b) => b.capabilities(),
        BackendKind::Slack { .. } => slakio_slack::adapter::CAPABILITIES,
    });
    if let Some(caps) = capabilities {
        app.connect(caps);
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
        app.warn(config_message(&e), Instant::now());
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
            BackendKind::Demo(b) => exchange::serve(b),
            BackendKind::Slack { credentials, endpoints } => wiring.serve(credentials, endpoints),
        });
        let saved = Saved { config: config_path.filter(|_| writable), errors: paths.errors_log() };
        run(&mut terminal, app, backend, &mut stats, saved, &wiring).await
    });
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
    /// Slack, read-only, for the signed-in workspace.
    Slack { credentials: Credentials, endpoints: Endpoints },
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
    test_addr: Option<std::net::SocketAddr>,
    /// What the sign-in screen's desktop scan found (no secret but the tokens; the cookie is
    /// read only when a workspace is picked).
    desktop: Option<Arc<slakio_auth::Found>>,
}

/// `SLAKIO_TEST_SLACK_ADDR`: a loopback address the tests' fake Slack listens on, used instead
/// of the workspace's hosts. Debug builds only; a release build never reads it.
fn test_addr() -> Option<std::net::SocketAddr> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var("SLAKIO_TEST_SLACK_ADDR").ok()?.parse().ok().filter(|a: &std::net::SocketAddr| a.ip().is_loopback())
}

/// A pasted request that signed in: the workspace kept, and how to reach it.
struct SignedIn {
    account: Account,
    credentials: Credentials,
    endpoints: Endpoints,
}

impl Wiring {
    fn endpoints(&self, host: &str) -> Option<Endpoints> {
        match self.test_addr {
            Some(addr) => Endpoints::local(addr),
            None => Endpoints::workspace(host),
        }
    }

    /// The signed-in workspace's credentials and endpoints; `None` when none is signed in.
    fn signed_in(&self) -> Result<Option<(Credentials, Endpoints)>, slakio_core::fault::Fault> {
        let Some(path) = &self.workspaces else { return Ok(None) };
        let Some(a) = account::load(path)?.into_iter().next() else { return Ok(None) };
        let Some(endpoints) = self.endpoints(&a.host) else { return Ok(None) };
        Ok(account::credentials(&*self.store, &a)?.map(|c| (c, endpoints)))
    }

    fn serve(&self, credentials: Credentials, endpoints: Endpoints) -> BackendHandle {
        slakio_slack::serve(slakio_slack::Client::new(credentials, endpoints, Arc::clone(&self.governor)))
    }

    /// Read `text`, ask Slack whether it signs in (one call), and keep it; or the words for why
    /// not. Nothing is kept unless Slack accepted it.
    async fn sign_in(self, text: Pasted) -> Result<SignedIn, Label> {
        let pasted = slakio_auth::parse(&text.0).map_err(paste_label)?;
        drop(text);
        self.finish(pasted.credentials, pasted.host).await
    }

    /// Sign in with the picked desktop workspace `team` (matched by team id in what the sign-in
    /// screen's scan found, never by position): read its credentials (the macOS keychain may
    /// prompt, off the loop's thread), then check and keep them.
    async fn sign_in_desktop(self, team: String) -> Result<SignedIn, Label> {
        let found = self.desktop.clone().ok_or(Label::SigninDesktopFailed)?;
        let ws = found.workspaces().iter().find(|w| w.team == team).ok_or(Label::SigninDesktopFailed)?;
        let host = ws.host.clone();
        let creds = tokio::task::spawn_blocking(move || found.system_credentials(&team))
            .await
            .map_err(|_| Label::SigninDesktopFailed)?
            .map_err(desktop_label)?;
        self.finish(creds, host).await
    }

    /// Check `credentials` for the workspace at `host` with one `client.userBoot` and keep them
    /// if Slack accepts; the words for why not otherwise. Nothing is kept unless Slack accepted.
    async fn finish(self, credentials: Credentials, host: String) -> Result<SignedIn, Label> {
        let endpoints = self.endpoints(&host).ok_or(Label::SigninNotWorkspace)?;
        let client = slakio_slack::Client::new(credentials.clone(), endpoints.clone(), Arc::clone(&self.governor));
        let who = login::check(&client).await.map_err(|e| {
            self.record(&slakio_core::fault::Fault::other(e.detail()));
            check_label(&e)
        })?;
        let account =
            Account { name: who.team_name.unwrap_or_else(|| who.team.clone()), team: who.team, host, user: who.user };
        let path = self.workspaces.clone().ok_or(Label::SigninStoreFailed)?;
        // The keychain can ask the user (a window of macOS): off the loop's thread.
        let (store, kept) = (Arc::clone(&self.store), account.clone());
        let creds = credentials.clone();
        let stored = tokio::task::spawn_blocking(move || account::keep(&*store, &path, &kept, &creds)).await;
        match stored {
            Ok(Ok(())) => Ok(SignedIn { account, credentials, endpoints }),
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

/// Offer signing in: when the real keychain is in use and nothing failed yet, try the Slack
/// desktop app first and show its workspaces as a picker; otherwise (or when it finds nothing)
/// fall back to the paste screen, with a short reason.
fn offer_sign_in_screen(app: &mut App, desktop_ok: bool, why: Option<Label>) -> Option<slakio_auth::Found> {
    use slakio_auth::DesktopError;
    if why.is_some() || !desktop_ok {
        app.offer_sign_in(why);
        return None;
    }
    // Before the terminal is set up: the scan reads files only (no keychain, no network).
    match slakio_auth::desktop::scan() {
        Ok(found) => {
            app.offer_desktop(
                found
                    .workspaces()
                    .iter()
                    .map(|w| PickerRow { team: w.team.clone(), name: shown(&w.name), host: shown(&w.host) })
                    .collect(),
            );
            return Some(found);
        }
        Err(DesktopError::NotSupported) => app.offer_sign_in(Some(Label::SigninDesktopUnsupported)),
        Err(e) if e.is_absent() => app.offer_sign_in(Some(Label::SigninDesktopMissing)),
        Err(_) => app.offer_sign_in(Some(Label::SigninDesktopFailed)),
    }
    None
}

/// The words for a desktop import that could not sign in.
fn desktop_label(e: slakio_auth::DesktopError) -> Label {
    use slakio_auth::DesktopError;
    match e {
        DesktopError::NotSupported => Label::SigninDesktopUnsupported,
        DesktopError::NoSecretService => Label::SigninDesktopKeychain,
        DesktopError::CookieLocked => Label::SigninDesktopLocked,
        _ if e.is_absent() => Label::SigninDesktopMissing,
        _ => Label::SigninDesktopFailed,
    }
}

/// The words for a paste that cannot sign in.
fn paste_label(e: PasteError) -> Label {
    match e {
        PasteError::Empty => Label::SigninEmpty,
        PasteError::TooLong => Label::SigninTooLong,
        PasteError::NoAddress => Label::SigninNoAddress,
        PasteError::NotWorkspace => Label::SigninNotWorkspace,
        PasteError::NoToken => Label::SigninNoToken,
        PasteError::NoCookie => Label::SigninNoCookie,
        PasteError::Ambiguous => Label::SigninAmbiguous,
    }
}

/// The words for a sign-in Slack did not accept, or could not be asked about.
fn check_label(e: &SlackError) -> Label {
    match e {
        SlackError::Auth { .. } => Label::SigninRefused,
        SlackError::Network { .. } => Label::SigninNetwork,
        SlackError::Offline { .. } => Label::SigninOffline,
        _ => Label::SigninFailed,
    }
}

/// A paste was checked: with a sign-in the app connects to its workspace, else says why not.
fn signed_in(done: Result<SignedIn, Label>, app: &mut App, backend: &mut Option<BackendHandle>, wiring: &Wiring) {
    match done {
        Ok(s) => {
            *backend = Some(wiring.serve(s.credentials, s.endpoints));
            app.signed_in(slakio_slack::adapter::CAPABILITIES, shown(&s.account.name), Instant::now());
        }
        Err(why) => app.sign_in_failed(why),
    }
}

/// A workspace's name as Slack gave it, safe to print.
fn shown(name: &str) -> String {
    Remote::new(name).line().into_string()
}

/// `slakio login` with the paste on standard input.
fn sign_in_piped(wiring: &Wiring, i18n: &i18n::I18n) -> ExitCode {
    let mut text = Pasted(String::new());
    let read = io::stdin().take(slakio_auth::curl::PASTE_MAX as u64 + 1).read_to_string(&mut text.0);
    let result = match read {
        Err(_) => Err(Label::SigninEmpty),
        Ok(_) => match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt.block_on(wiring.clone().sign_in(text)),
            Err(_) => Err(Label::SigninFailed),
        },
    };
    match result {
        Ok(s) => {
            println!("{}", i18n.msg(&Msg::SigninSignedIn { workspace: shown(&s.account.name) }));
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("{}", i18n.label(why));
            ExitCode::FAILURE
        }
    }
}

/// `slakio logout`: every signed-in workspace's credentials leave the store, then the file.
fn sign_out(
    store: &dyn SecretStore,
    path: Option<&std::path::Path>,
    i18n: &i18n::I18n,
    errors: Option<PathBuf>,
) -> ExitCode {
    let Some(path) = path else {
        println!("{}", i18n.label(Label::LogoutNone));
        return ExitCode::SUCCESS;
    };
    match account::forget(store, path) {
        Ok(gone) if gone.is_empty() => println!("{}", i18n.label(Label::LogoutNone)),
        Ok(gone) => {
            for a in gone {
                println!("{}", i18n.msg(&Msg::LogoutDone { workspace: shown(&a.name) }));
            }
        }
        Err(fault) => {
            ErrorLog::new(errors).record("logout", &fault);
            eprintln!("{}", i18n.label(Label::LogoutFailed));
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// The status line's words for a config file that cannot be used.
fn config_message(e: &ConfigError) -> Msg {
    match e {
        ConfigError::Read { .. } => Msg::Label(Label::ConfigReadFailed),
        ConfigError::Syntax(_) => Msg::Label(Label::ConfigSyntax),
        ConfigError::UnknownKey(key) => Msg::ConfigUnknownKey { key: key.clone() },
        ConfigError::Value { key, value, allowed } => {
            Msg::ConfigBadValue { key: key.clone(), value: value.clone(), allowed: allowed.clone() }
        }
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
    wiring: &Wiring,
) -> io::Result<()> {
    let size = terminal.size()?;
    app.resize(size.width, size.height);
    let mut events = EventStream::new();
    let mut signals = Signals::new()?;
    let mut cursor = Cursor::default();
    let mut redraw = true;
    // A paste being checked with Slack.
    let mut signing_in: Option<tokio::task::JoinHandle<Result<SignedIn, Label>>> = None;
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
                Effect::SignIn(pasted) => signing_in = Some(tokio::spawn(wiring.clone().sign_in(pasted))),
                Effect::DesktopPick(team) => signing_in = Some(tokio::spawn(wiring.clone().sign_in_desktop(team))),
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
                signed_in(done.unwrap_or(Err(Label::SigninFailed)), &mut app, &mut backend, wiring);
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

/// The signals that end the process (Unix): SIGTERM, SIGHUP (the terminal went away), SIGINT
/// (sent from outside: in raw mode Ctrl+C is a key) and SIGQUIT. Once caught they no longer
/// kill the process on the spot, so the terminal is restored first.
struct Signals {
    #[cfg(unix)]
    all: Vec<tokio::signal::unix::Signal>,
}

impl Signals {
    fn new() -> io::Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let kinds = [SignalKind::terminate(), SignalKind::hangup(), SignalKind::interrupt(), SignalKind::quit()];
            Ok(Self { all: kinds.into_iter().map(signal).collect::<io::Result<Vec<_>>>()? })
        }
        #[cfg(not(unix))]
        Ok(Self {})
    }

    /// The next of them.
    async fn recv(&mut self) {
        #[cfg(unix)]
        {
            let each = self.all.iter_mut().map(|s| Box::pin(s.recv()));
            futures::future::select_all(each).await;
        }
        #[cfg(not(unix))]
        std::future::pending::<()>().await
    }
}
