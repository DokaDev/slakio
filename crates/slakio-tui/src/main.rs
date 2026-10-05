//! `slakio` binary: parses the command line, owns the terminal and runs the event loop.

mod cli;
mod stats;
mod term;

use cli::{Cli, HELP, parse_args};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::EventStream;
use slakio_core::config::{self, ConfigError};
use slakio_core::fault::ErrorLog;
use slakio_core::i18n::{self, Label, Msg};
use slakio_core::paths::Paths;
use slakio_core::secret::{STORE_ENV, StoreKind};
use slakio_tui::app::App;
use slakio_tui::terminal::{Cursor, cursor_shape};
use slakio_tui::theme::Theme;
use slakio_tui::ui;
use std::io::{self, Stdout};
use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    let mut stats = stats::Stats::from_env();
    let cli_config = match parse_args(std::env::args_os().skip(1)) {
        Ok(Cli::Run { config }) => config,
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
    // Before anything else: which credential store this process may touch. Nothing opens it
    // yet, but a value that would not be understood stops the program here, never later.
    if let Err(e) = StoreKind::from_env(std::env::var(STORE_ENV).ok().as_deref()) {
        eprintln!("{e}");
        return ExitCode::from(2);
    }
    let paths = Paths::from_env();
    let config_path = cli_config.or_else(|| paths.config_file());
    let (cfg, cfg_err) = config::load(config_path.as_deref());
    let lang = i18n::detect_lang(&cfg.language, |k| std::env::var(k).ok());
    let mut app = App::new(lang, Theme::from_env(|k| std::env::var(k).ok()));
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
        let mut terminal = term::setup_terminal()?;
        run(&mut terminal, app, &mut stats).await
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

/// The event loop: draw when something changed, then sleep until the next input event, signal
/// or deadline of the app. With nothing to do it never wakes up.
async fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    mut app: App,
    stats: &mut Option<stats::Stats>,
) -> io::Result<()> {
    let mut events = EventStream::new();
    let mut signals = Signals::new()?;
    let mut cursor = Cursor::default();
    let mut redraw = true;
    // Debug builds only: panic after the first frame, for the test that the panic hook
    // restores the terminal (tests/signals.rs).
    let panic_after_frame = cfg!(debug_assertions) && std::env::var_os("SLAKIO_DEBUG_PANIC").is_some_and(|v| v == "1");
    while !app.quit {
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
