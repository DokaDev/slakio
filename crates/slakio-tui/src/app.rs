//! The application state. `App` is a thin router: each part of the state lives in a sub-state
//! that owns its data and its update ([`cmdline::CommandLine`], [`status::Status`],
//! [`shell::Shell`], [`model::Model`]); `App` turns input into [`Action`]s through the key map
//! and routes each action to its owner.
//!
//! State does no I/O. Requests to the backend are queued ([`App::take_commands`]) and its
//! answers handed in ([`App::on_backend`]) by the binary's loop, which also reads [`App::quit`]
//! and [`App::deadline`]. Without a backend (`slakio` without `--demo`, for now) the work area
//! only says how to quit and how to try the demo.

pub mod cmdline;
pub mod model;
pub mod shell;
pub mod status;

use crate::action::{self, Action, AppAction, CommandLineAction, ShellAction};
use crate::keymap::{Ctx, KeyChord, KeyState, Keymap, Resolved};
use crate::screen;
use crate::theme::Theme;
use model::Model;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use shell::{Region, Shell, rail_items};
use slakio_core::backend::{Capabilities, Command, Envelope, Event as BackendEvent, Generation};
use slakio_core::i18n::{I18n, Lang, Msg};
use status::{Level, Status};
use std::time::Instant;

/// The input mode, shown by the badge at the left of the status line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Command,
}

/// Display settings from the config file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// Nerd Font icons instead of letters.
    pub icons: bool,
    /// The expanded rail pushes the list panel aside instead of covering it.
    pub rail_push: bool,
}

pub struct App {
    pub keymap: Keymap,
    /// Keys of an unfinished sequence (`Space w …`).
    pub keys: KeyState,
    pub i18n: I18n,
    pub theme: Theme,
    pub settings: Settings,
    pub cmdline: cmdline::CommandLine,
    pub status: Status,
    pub shell: Shell,
    pub model: Model,
    /// What the connected backend can do; `None` without one.
    pub backend: Option<Capabilities>,
    /// The generation of the last boot request; older answers are dropped.
    boot: Generation,
    commands: Vec<(Generation, Command)>,
    /// The terminal's size, for the mouse and scrolling.
    pub size: Rect,
    /// Set once the user asked to quit; the binary's loop ends.
    pub quit: bool,
}

impl App {
    pub fn new(lang: Lang, theme: Theme) -> Self {
        Self {
            keymap: Keymap::default(),
            keys: KeyState::default(),
            i18n: I18n::new(lang),
            theme,
            settings: Settings::default(),
            cmdline: cmdline::CommandLine::default(),
            status: Status::default(),
            shell: Shell::default(),
            model: Model::default(),
            backend: None,
            boot: Generation::default(),
            commands: Vec::new(),
            size: Rect::default(),
            quit: false,
        }
    }

    /// Use a backend that can do `caps`: asks it for the workspaces.
    pub fn connect(&mut self, caps: Capabilities) {
        self.backend = Some(caps);
        self.boot = self.boot.next();
        self.commands.push((self.boot, Command::Boot));
    }

    /// The requests for the backend queued since the last call.
    pub fn take_commands(&mut self) -> Vec<(Generation, Command)> {
        std::mem::take(&mut self.commands)
    }

    /// An event of the backend. `true` when the screen changed; an answer to an older request
    /// is dropped.
    pub fn on_backend(&mut self, envelope: Envelope) -> bool {
        match envelope.event {
            BackendEvent::Booted(snapshot) if envelope.generation == self.boot => {
                self.model = Model::new(*snapshot);
                self.shell.clamp(&self.model, self.list_height());
                true
            }
            BackendEvent::Booted(_) => false,
        }
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.size = Rect::new(0, 0, width, height);
        self.shell.scroll(self.list_height());
    }

    pub fn mode(&self) -> Mode {
        if self.cmdline.is_open() { Mode::Command } else { Mode::Normal }
    }

    /// Where the keyboard is.
    pub fn key_context(&self) -> Ctx {
        if self.cmdline.is_open() {
            return Ctx::CommandLine;
        }
        if self.backend.is_none() {
            return Ctx::Root;
        }
        match self.shell.focus {
            Region::Rail => Ctx::Rail,
            Region::List => Ctx::List,
            Region::Work => Ctx::PaneNormal,
        }
    }

    /// The screen's areas now.
    pub fn areas(&self) -> screen::Areas {
        screen::areas(self.size, self.shell.rail_expanded(), self.settings.rail_push, self.shell.list_hidden)
    }

    /// Rows the list panel shows.
    fn list_height(&self) -> usize {
        self.areas().list.map_or(1, |l| usize::from(screen::inner(l).height))
    }

    /// Show a warning in the status line.
    pub fn warn(&mut self, msg: impl Into<Msg>, now: Instant) {
        self.status.show(msg, Level::Warning, now);
    }

    /// Handle a terminal event at `now`. `true` when the screen may have changed.
    pub fn handle_event(&mut self, ev: Event, now: Instant) -> bool {
        match ev {
            // Release and repeat events are reported only with the kitty flags this app never
            // asks for; a press is what counts.
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                let key = KeyChord::from_event(&k);
                let ctx = self.key_context();
                match self.keymap.feed(&mut self.keys, ctx, key) {
                    Resolved::Action(a) => self.dispatch(a, now),
                    Resolved::Pending => {}
                    Resolved::Unbound(seq) => {
                        if let [key] = seq[..] {
                            self.type_key(ctx, key);
                        }
                    }
                }
                true
            }
            Event::Paste(text) => {
                self.cmdline.insert(&text);
                true
            }
            Event::Resize(w, h) => {
                self.resize(w, h);
                true
            }
            Event::Mouse(m) => self.mouse(m),
            _ => false,
        }
    }

    /// A key no binding claims, for what has the focus.
    fn type_key(&mut self, ctx: Ctx, key: KeyChord) {
        if ctx != Ctx::CommandLine {
            return;
        }
        match key.code {
            KeyCode::Char(c) if !key.mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                self.cmdline.insert(c.encode_utf8(&mut [0; 4]));
            }
            KeyCode::Backspace => self.cmdline.backspace(),
            _ => {}
        }
    }

    /// The mouse: hovering the rail expands it; a click on a rail item shows it, on a list row
    /// opens it, on the work area focuses it. `true` when the screen changed.
    fn mouse(&mut self, m: MouseEvent) -> bool {
        if self.backend.is_none() || self.cmdline.is_open() || screen::too_small(self.size) {
            return false;
        }
        let at = Position { x: m.column, y: m.row };
        let a = self.areas();
        match m.kind {
            MouseEventKind::Moved => {
                let hover = a.rail.contains(at);
                let changed = hover != self.shell.hover_rail;
                self.shell.hover_rail = hover;
                changed
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.keys.clear();
                if a.rail.contains(at) {
                    let items = rail_items(self.model.workspaces().len());
                    if let Some(i) = screen::rail_item_at(a.rail, self.model.workspaces().len(), items.len(), at.y) {
                        self.shell.select(items[i], &items);
                    }
                } else if let Some(list) = a.list.filter(|l| l.contains(at)) {
                    let inner = screen::inner(list);
                    if inner.contains(at) {
                        let row = self.shell.list_top + usize::from(at.y - inner.y);
                        if row < self.shell.rows(&self.model).len() {
                            self.shell.focus = Region::List;
                            self.shell.list_cursor = row;
                            self.shell.open_row(&self.model);
                        }
                    }
                } else if a.work.contains(at) {
                    self.shell.focus = Region::Work;
                }
                true
            }
            _ => false,
        }
    }

    /// Route `action` to the sub-state that owns it.
    pub fn dispatch(&mut self, action: Action, now: Instant) {
        match action {
            Action::App(AppAction::Quit) => self.quit = true,
            Action::CommandLine(a) => self.command_line(a, now),
            Action::Shell(a) => self.shell(a),
        }
    }

    fn shell(&mut self, a: ShellAction) {
        if self.backend.is_some() {
            let height = self.list_height();
            self.shell.update(a, &self.model, height);
        }
    }

    fn command_line(&mut self, a: CommandLineAction, now: Instant) {
        match a {
            CommandLineAction::Open => self.cmdline.open(),
            CommandLineAction::Cancel => self.cmdline.close(),
            CommandLineAction::Run => {
                let text = self.cmdline.take();
                let name = text.trim();
                if name.is_empty() {
                    return;
                }
                match action::by_command(name) {
                    Some(a) => self.dispatch(a, now),
                    None => self.warn(Msg::CommandUnknown { name: name.to_string() }, now),
                }
            }
        }
    }

    /// When the screen changes next by itself, if ever (the loop sleeps until then).
    pub fn deadline(&self) -> Option<Instant> {
        self.status.deadline()
    }

    /// The deadline passed. `true` when the screen changed.
    pub fn on_tick(&mut self, now: Instant) -> bool {
        self.status.expire(now)
    }
}
