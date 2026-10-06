//! The application state. `App` is a thin router: each part of the state lives in a sub-state
//! that owns its data and its update ([`cmdline::CommandLine`] with its [`palette`],
//! [`status::Status`],
//! [`shell::Shell`], [`work::Work`] with its [`pane::Pane`]s and their [`composer::Composer`]s,
//! [`model::Model`], the keyboard [`help::Help`] and a [`dialog::Dialog`]); `App` turns input
//! into [`Action`]s through the key map and routes each action to its owner.
//!
//! State does no I/O. Requests to the backend are queued ([`App::take_commands`]) and its
//! answers handed in ([`App::on_backend`]); what only the binary can do (put text on the
//! clipboard, save a setting) is queued as an [`Effect`] ([`App::take_effects`]). The binary's
//! loop delivers both and also reads [`App::quit`] and [`App::deadline`]. Without a backend
//! (`slakio` without `--demo`, for now) the work area only says how to quit and how to try the
//! demo.
//!
//! Where the focus lands, and what `Esc` does, one step at a time:
//!
//! | from | `Esc` | `Ctrl+W` |
//! |---|---|---|
//! | VISUAL | Normal, the selection stays | |
//! | a selected message | no selection (back to the newest) | |
//! | the thread panel | the main pane | closes it; the main pane selects its message |
//! | the main pane | the list, on its conversation | closes it; the list, on its conversation |
//! | the rail | the list | |
//!
//! The rail is reached from anywhere outside text with `Ctrl+R` or `Space r`, and is a stop of
//! the `Tab` round (left of the list).
//! | the list | nothing | |

pub mod cmdline;
pub mod composer;
pub mod dialog;
pub mod help;
mod layout;
pub mod model;
pub mod palette;
pub mod pane;
pub mod query;
pub mod shell;
pub mod status;
pub(crate) mod work;

use crate::action::{
    Action, AppAction, CommandLineAction, ComposerAction, DialogAction, HelpAction, PaneAction, ShellAction,
};
use crate::input::hangul;
use crate::keymap::{Ctx, KeyChord, KeyState, Keymap, Resolved};
use crate::screen;
use crate::theme::{self, Look, Theme};
use dialog::{Dialog, Question};
use help::Help;
use model::Model;
pub use query::{Focus, Overlay, PaneKind, PaneRef};
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use shell::{Region, Shell, rail_items};
use slakio_core::backend::{Capabilities, Command, Envelope, Event as BackendEvent, Generation};
use slakio_core::i18n::{I18n, Label, Lang, Msg};
use status::{Level, Status};
use std::time::{Duration, Instant};
use work::{Side, Work};

/// How long an unfinished key sequence waits before the which-key popup lists what may follow
/// (typed quickly, the popup never shows).
pub const WHICH_KEY_DELAY: Duration = Duration::from_millis(300);
/// Two clicks on the same row within this time are a double click.
pub const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// Rows the mouse wheel scrolls the list by.
const WHEEL_ROWS: isize = 3;

/// The input mode, shown by the badge at the left of the status line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// Writing in a composer.
    Insert,
    /// Selecting a range of messages.
    Visual,
    Command,
}

/// Something only the binary can do, done by its loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Put text on the system clipboard (OSC 52; the terminal does it, no clipboard library
    /// is touched).
    Copy(String),
    /// Save `key = value` in the config file (an answer the app asked for).
    Save { key: &'static str, value: String },
}

/// Display settings from the config file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Nerd Font icons instead of letters.
    pub icons: bool,
    /// The expanded rail pushes the list panel aside instead of covering it.
    pub rail_push: bool,
    /// People are pictured by an initials chip (`avatars = "initials"`, the default).
    pub avatars: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { icons: false, rail_push: false, avatars: true }
    }
}

/// The unfinished key sequence's popup: when it shows, and whether it does yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Guide {
    at: Option<Instant>,
    shown: bool,
}

pub struct App {
    pub keymap: Keymap,
    /// Keys of an unfinished sequence (`Space w …`).
    pub keys: KeyState,
    guide: Guide,
    pub i18n: I18n,
    pub theme: Theme,
    /// The `theme` setting as written (`auto`, a family or a theme), and what decides how it is
    /// drawn here; `:theme` changes the one and keeps the other.
    pub theme_setting: String,
    pub look: Look,
    pub settings: Settings,
    pub cmdline: cmdline::CommandLine,
    pub status: Status,
    pub shell: Shell,
    pub(crate) work: Work,
    pub model: Model,
    /// The keyboard help, while open.
    pub help: Option<Help>,
    /// A question, while asked.
    pub dialog: Option<Dialog>,
    /// What the backend can do; `None` without one.
    pub backend: Option<Capabilities>,
    /// The generation of the last boot request; older answers are dropped.
    boot: Generation,
    commands: Vec<(Generation, Command)>,
    effects: Vec<Effect>,
    /// The last click (for a double click): when, and where.
    last_click: Option<(Instant, Position)>,
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
            guide: Guide::default(),
            i18n: I18n::new(lang),
            theme,
            theme_setting: "auto".to_string(),
            look: Look::default(),
            settings: Settings::default(),
            cmdline: cmdline::CommandLine::default(),
            status: Status::default(),
            shell: Shell::default(),
            work: Work::default(),
            model: Model::default(),
            help: None,
            dialog: None,
            backend: None,
            boot: Generation::default(),
            commands: Vec::new(),
            effects: Vec::new(),
            last_click: None,
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

    /// Ask once whether the terminal shows Nerd Font icons; the rail previews the answer that
    /// has the focus, and the answer is saved ([`Effect::Save`]).
    pub fn ask_icons(&mut self) {
        self.dialog = Some(Dialog::new(Question::Icons));
        self.settings.icons = false;
    }

    /// The requests for the backend queued since the last call.
    pub fn take_commands(&mut self) -> Vec<(Generation, Command)> {
        let mut out = std::mem::take(&mut self.commands);
        out.extend(self.work.take_requests());
        out
    }

    /// The effects queued since the last call.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// An event of the backend. `true` when the screen changed; an answer to an older request
    /// is dropped.
    pub fn on_backend(&mut self, envelope: Envelope) -> bool {
        match envelope.event {
            BackendEvent::Booted(snapshot) if envelope.generation == self.boot => {
                self.model = Model::new(*snapshot);
                self.shell.clamp(&self.model, self.list_height());
                self.work.clamp(&self.model);
                true
            }
            BackendEvent::Booted(_) => false,
            BackendEvent::History(page) => self.work.on_page(envelope.generation, &page, &self.model),
        }
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.size = Rect::new(0, 0, width, height);
        self.shell.scroll(self.list_height());
    }

    pub fn mode(&self) -> Mode {
        match self.key_context() {
            Ctx::CommandLine => Mode::Command,
            Ctx::ComposerInsert => Mode::Insert,
            Ctx::PaneVisual => Mode::Visual,
            _ => Mode::Normal,
        }
    }

    /// Where the keyboard is.
    pub fn key_context(&self) -> Ctx {
        if self.dialog.is_some() {
            return Ctx::Dialog;
        }
        if let Some(h) = &self.help {
            return if h.typing { Ctx::HelpFilter } else { Ctx::Help };
        }
        self.screen_context()
    }

    /// Where the keyboard is on the screen under any popup.
    pub fn screen_context(&self) -> Ctx {
        if self.cmdline.is_open() {
            return Ctx::CommandLine;
        }
        self.region_context()
    }

    /// Where the keyboard is in the regions of the screen, under the command line too (the keys
    /// the palette shows are those of this context).
    pub fn region_context(&self) -> Ctx {
        if self.backend.is_none() {
            return Ctx::Root;
        }
        match self.shell.focus {
            Region::Rail => Ctx::Rail,
            Region::List => Ctx::List,
            Region::Work if self.work.insert && self.work.focused().is_some() => Ctx::ComposerInsert,
            Region::Work if self.work.focused().is_some_and(|p| p.visual.is_some()) => Ctx::PaneVisual,
            Region::Work => Ctx::PaneNormal,
        }
    }

    /// Rows the list panel shows (or would show, while it is hidden).
    fn list_height(&self) -> usize {
        let a = self.areas();
        let panel = a.list.unwrap_or(a.work);
        usize::from(screen::inner(panel).height).max(1)
    }

    /// Rows of messages the focused pane shows.
    fn pane_height(&self) -> usize {
        let frame = self.frame();
        frame.pane(self.work.side.slot()).map_or(1, |p| usize::from(p.parts.messages.height)).max(1)
    }

    /// The which-key popup shows at `now`.
    pub fn which_key_visible(&self, now: Instant) -> bool {
        !self.keys.pending().is_empty() && self.guide.at.is_some_and(|at| now >= at)
    }

    /// Show a warning in the status line.
    pub fn warn(&mut self, msg: impl Into<Msg>, now: Instant) {
        self.status.show(msg, Level::Warning, now);
    }

    fn info(&mut self, msg: impl Into<Msg>, now: Instant) {
        self.status.show(msg, Level::Info, now);
    }

    /// Handle a terminal event at `now`. `true` when the screen may have changed.
    pub fn handle_event(&mut self, ev: Event, now: Instant) -> bool {
        match ev {
            // Release and repeat events are reported only with the kitty flags this app never
            // asks for; a press is what counts.
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                let key = KeyChord::from_event(&k);
                // Outside text input, Hangul typed with a Korean input source means the QWERTY
                // keys at the same places (`j` arrives as the jamo on that key); a syllable is
                // several keys, each resolved where the previous one left off.
                let mapped = match key.code {
                    KeyCode::Char(c) if key.mods.is_empty() && !self.key_context().is_text_input() => hangul::keys(c),
                    _ => None,
                };
                match mapped {
                    Some(keys) => {
                        for k in keys {
                            self.feed(KeyChord::char(k), now);
                        }
                    }
                    None => self.feed(key, now),
                }
                true
            }
            Event::Paste(text) => {
                match self.key_context() {
                    Ctx::ComposerInsert => {
                        if let Some(p) = self.work.focused_mut() {
                            p.composer.insert(&text);
                        }
                    }
                    Ctx::HelpFilter => {
                        if let Some(h) = self.help.as_mut() {
                            h.filter.extend(text.chars().filter(|c| !c.is_control()));
                            h.cursor = 0;
                        }
                    }
                    _ => self.cmdline.insert(&text),
                }
                true
            }
            Event::Resize(w, h) => {
                self.resize(w, h);
                true
            }
            Event::Mouse(m) => self.mouse(m, now),
            _ => false,
        }
    }

    /// Resolve one key in the current context.
    fn feed(&mut self, key: KeyChord, now: Instant) {
        let ctx = self.key_context();
        // While a sequence waits, `Backspace` takes its last key back (the popup goes up a
        // level); a `?` that ends no sequence shows every key (below).
        if !self.keys.pending().is_empty()
            && !ctx.is_text_input()
            && key.code == KeyCode::Backspace
            && key.mods.is_empty()
        {
            self.keys.back();
            if self.keys.pending().is_empty() {
                self.guide = Guide::default();
            }
            return;
        }
        let pending = self.keys.pending().len();
        match self.keymap.feed(&mut self.keys, ctx, key) {
            Resolved::Action(a) => {
                self.guide = Guide::default();
                self.dispatch(a, now);
            }
            Resolved::Pending => {
                if pending == 0 {
                    self.guide = Guide { at: Some(now + WHICH_KEY_DELAY), shown: false };
                }
            }
            Resolved::Unbound(seq) => {
                self.guide = Guide::default();
                match seq[..] {
                    [key] => self.type_key(ctx, key),
                    [.., last] if last == KeyChord::char('?') => self.open_help(ctx),
                    _ => {}
                }
            }
        }
    }

    /// A key no binding claims, for what has the focus.
    fn type_key(&mut self, ctx: Ctx, key: KeyChord) {
        let typed = match key.code {
            KeyCode::Char(c) if !key.mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => Some(c),
            _ => None,
        };
        match ctx {
            Ctx::ComposerInsert => {
                let Some(c) = self.work.focused_mut().map(|p| &mut p.composer) else { return };
                match (typed, key.code) {
                    (Some(ch), _) => c.insert(ch.encode_utf8(&mut [0; 4])),
                    (_, KeyCode::Backspace) => {
                        c.backspace();
                    }
                    (_, KeyCode::Delete) => {
                        c.delete();
                    }
                    (_, KeyCode::Left) => c.left(),
                    (_, KeyCode::Right) => c.right(),
                    (_, KeyCode::Up) => c.vertical(-1),
                    (_, KeyCode::Down) => c.vertical(1),
                    (_, KeyCode::Home) => c.home(),
                    (_, KeyCode::End) => c.end(),
                    _ => {}
                }
            }
            Ctx::CommandLine => match (typed, key.code) {
                (Some(c), _) => self.cmdline.insert(c.encode_utf8(&mut [0; 4])),
                (_, KeyCode::Backspace) => self.cmdline.backspace(),
                _ => {}
            },
            Ctx::HelpFilter => {
                let Some(h) = self.help.as_mut() else { return };
                match (typed, key.code) {
                    (Some(c), _) => h.filter.push(c),
                    (_, KeyCode::Backspace) if h.filter.is_empty() => h.typing = false,
                    (_, KeyCode::Backspace) => {
                        h.filter.pop();
                    }
                    _ => {}
                }
                h.cursor = 0;
            }
            _ => {}
        }
    }

    /// The mouse: hovering the rail expands it; a click on a rail item shows it, on a list row
    /// opens it, on a message selects it (twice: its thread), on a reply link opens the thread,
    /// on a composer writes in it; the wheel scrolls what is under it. `true` when the screen
    /// changed.
    fn mouse(&mut self, m: MouseEvent, now: Instant) -> bool {
        if self.cmdline.is_open() && self.dialog.is_none() {
            return self.palette_mouse(m, now);
        }
        if self.backend.is_none() || self.dialog.is_some() || screen::too_small(self.size) {
            return false;
        }
        let at = Position { x: m.column, y: m.row };
        if let Some(h) = self.help.as_mut() {
            let by = match m.kind {
                MouseEventKind::ScrollDown => 1,
                MouseEventKind::ScrollUp => -1,
                _ => return false,
            };
            let n = h.rows(&self.keymap, &self.i18n).len();
            h.step(by, n);
            return true;
        }
        let a = self.areas();
        match m.kind {
            MouseEventKind::Moved => {
                let hover = a.rail.contains(at);
                let n = self.model.workspaces().len();
                let item = hover.then(|| screen::rail_item_at(a.rail, n, rail_items(n).len(), at.y)).flatten();
                let changed = hover != self.shell.hover_rail || item != self.shell.hover_item;
                self.shell.hover_rail = hover;
                self.shell.hover_item = item;
                changed
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let by = if m.kind == MouseEventKind::ScrollDown { 1 } else { -1 };
                if a.list.is_some_and(|l| l.contains(at)) {
                    let height = self.list_height();
                    self.shell.scroll_by(&self.model, by * WHEEL_ROWS, height);
                    return true;
                }
                let slot = self.frame().pane_at(at).map(|p| p.slot);
                if let Some(p) = slot.and_then(|s| self.work.pane_mut(s)) {
                    p.step(by);
                }
                self.work.with_pane(|_| {});
                true
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.keys.clear();
                self.guide = Guide::default();
                let double = self.last_click.is_some_and(|(t, p)| p == at && now.duration_since(t) <= DOUBLE_CLICK);
                self.last_click = Some((now, at));
                if a.rail.contains(at) {
                    let items = rail_items(self.model.workspaces().len());
                    if let Some(i) = screen::rail_item_at(a.rail, self.model.workspaces().len(), items.len(), at.y) {
                        self.shell.select(items[i], &items, &self.model);
                    }
                } else if let Some(list) = a.list.filter(|l| l.contains(at)) {
                    let inner = screen::inner(list);
                    if inner.contains(at) {
                        let row = self.shell.list_top + usize::from(at.y - inner.y);
                        if self.shell.rows(&self.model).get(row).is_some_and(|r| r.is_selectable()) {
                            self.shell.focus = Region::List;
                            self.shell.list_cursor = row;
                            if let Some(t) = self.shell.open_row(&self.model) {
                                self.open(t, true);
                            }
                        }
                    }
                } else if a.work.contains(at) {
                    self.click_work(at, double);
                }
                true
            }
            _ => false,
        }
    }

    /// A click in the work area at `at` (`double`: the second of a double click).
    fn click_work(&mut self, at: Position, double: bool) {
        let Some(&layout) = self.frame().pane_at(at) else { return };
        let side = Side::of(layout.slot);
        self.shell.focus = Region::Work;
        if side != self.work.side {
            self.work.side = side;
            self.work.insert = false;
        }
        let Some(pane) = self.work.focused() else { return };
        let parts = layout.parts;
        let hit = pane.hit(at.y).filter(|_| parts.messages.contains(at));
        self.work.insert = parts.input.contains(at);
        let Some(hit) = hit else { return };
        self.work.with_pane(|p| {
            p.selected = Some(hit.message);
            p.visual = None;
        });
        if (hit.link || double) && side == Side::Main {
            self.work.open_thread();
        }
    }

    /// Route `action` to the sub-state that owns it.
    pub fn dispatch(&mut self, action: Action, now: Instant) {
        match action {
            Action::App(a) => self.app(a, now),
            Action::CommandLine(a) => self.command_line(a, now),
            Action::Shell(a) => self.shell(a, now),
            Action::Pane(a) => self.pane(a, now),
            Action::Composer(a) => self.composer(a, now),
            Action::Help(a) => self.help(a, now),
            Action::Dialog(a) => self.answer(a),
        }
    }

    fn app(&mut self, a: AppAction, now: Instant) {
        match a {
            AppAction::Quit => match self.dialog {
                // Asked already: the question stays until it is answered.
                Some(d) if d.question == Question::Quit => {}
                _ if self.work.unsent() => self.confirm_quit(),
                _ => self.quit = true,
            },
            AppAction::Interrupt => {
                self.keys.clear();
                self.guide = Guide::default();
                let keys = crate::keymap::hints::key_label(&self.keymap, Action::App(AppAction::Quit), Ctx::Root)
                    .unwrap_or_else(|| ":qa".to_string());
                self.info(Msg::StatusQuitHint { keys }, now);
            }
            AppAction::Palette => {
                if self.dialog.is_none() {
                    self.help = None;
                    if self.cmdline.is_open() {
                        self.cmdline.close();
                    } else {
                        self.cmdline.open();
                    }
                }
            }
            AppAction::ToggleAvatars => {
                let value = if self.settings.avatars { "off" } else { "initials" };
                if let Err(msg) = self.set_avatars(value, now) {
                    self.warn(msg, now);
                }
            }
            AppAction::ChooseWorkspace => {
                if self.backend.is_some() {
                    self.shell.focus = Region::Rail;
                    self.shell.rail_cursor = self.shell.workspace;
                }
            }
        }
    }

    /// Ask before quitting: about the text not sent when a composer holds some, else only
    /// whether to quit (for a click, which is easy to make by mistake).
    fn confirm_quit(&mut self) {
        self.help = None;
        let q = if self.work.unsent() { Question::Quit } else { Question::QuitConfirm };
        self.dialog = Some(Dialog::new(q));
    }

    fn shell(&mut self, a: ShellAction, now: Instant) {
        if self.backend.is_none() {
            return;
        }
        match a {
            ShellAction::FocusNext => return self.cycle(1),
            ShellAction::FocusPrev => return self.cycle(-1),
            ShellAction::FocusUp => return self.info(Msg::Label(Label::StatusNoPaneAbove), now),
            ShellAction::FocusDown => return self.info(Msg::Label(Label::StatusNoPaneBelow), now),
            ShellAction::FocusLeft if self.shell.focus == Region::Work => return self.pane(PaneAction::Left, now),
            // Never onto an empty work area: nothing there takes a key.
            ShellAction::FocusRight if self.shell.focus == Region::List && self.work.main.is_none() => return,
            ShellAction::FocusRight if self.shell.focus == Region::Work => return self.pane(PaneAction::Right, now),
            _ => {}
        }
        let height = self.list_height();
        if let Some(open) = self.shell.update(a, &self.model, height) {
            self.open(open.target, open.focus);
        }
    }

    /// Move the focus to the next (`1`) or previous (`-1`) panel: rail, list, main pane, thread
    /// panel, round again. A hidden list or a closed pane is skipped. The rail is expanded only
    /// while it has the focus, so passing it never leaves it open.
    fn cycle(&mut self, step: isize) {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Stop {
            Rail,
            List,
            Main,
            Thread,
        }
        let mut stops = vec![Stop::Rail];
        if !self.shell.list_hidden {
            stops.push(Stop::List);
        }
        if self.work.main.is_some() {
            stops.push(Stop::Main);
        }
        if self.work.thread.is_some() {
            stops.push(Stop::Thread);
        }
        let here = match (self.shell.focus, self.work.side) {
            (Region::Rail, _) => Stop::Rail,
            (Region::Work, Side::Thread) => Stop::Thread,
            (Region::Work, Side::Main) => Stop::Main,
            _ => Stop::List,
        };
        let at = stops.iter().position(|s| *s == here);
        let n = stops.len() as isize;
        let to = match at {
            Some(i) => stops[(i as isize + step).rem_euclid(n) as usize],
            None => stops[0],
        };
        self.work.insert = false;
        match to {
            Stop::Rail => self.shell.focus = Region::Rail,
            Stop::List => self.shell.focus = Region::List,
            Stop::Main => {
                self.shell.focus = Region::Work;
                self.work.side = Side::Main;
            }
            Stop::Thread => {
                self.shell.focus = Region::Work;
                self.work.side = Side::Thread;
            }
        }
    }

    /// Open a conversation in the main pane; `focus`: give it the keyboard, as GUI Slack does
    /// (the next `j`/`k` moves through its messages).
    fn open(&mut self, target: slakio_core::model::Target, focus: bool) {
        self.work.open(target);
        if focus {
            self.shell.focus = Region::Work;
        }
    }

    /// The list panel gets the keyboard, its cursor on the conversation `target` names.
    fn focus_list(&mut self, target: Option<slakio_core::model::Target>) {
        self.work.insert = false;
        self.shell.list_hidden = false;
        self.shell.focus = Region::List;
        if let Some(t) = target {
            let height = self.list_height();
            self.shell.reveal(&self.model, &t, height);
        }
    }

    fn pane(&mut self, a: PaneAction, now: Instant) {
        if self.backend.is_none() {
            return;
        }
        let page = self.pane_height() as isize;
        let main_target = self.work.main.as_ref().map(|p| p.target.clone());
        match a {
            PaneAction::Back | PaneAction::Forward => {
                let moved = if a == PaneAction::Back { self.work.back() } else { self.work.forward() };
                if moved {
                    self.shell.focus = Region::Work;
                } else {
                    let l = if a == PaneAction::Back { Label::StatusNoEarlier } else { Label::StatusNoLater };
                    self.info(Msg::Label(l), now);
                }
            }
            PaneAction::Next => self.work.with_pane(|p| p.step(1)),
            PaneAction::Prev => self.work.with_pane(|p| p.step(-1)),
            PaneAction::HalfDown => self.work.with_pane(|p| p.step((page / 2).max(1))),
            PaneAction::HalfUp => self.work.with_pane(|p| p.step(-(page / 2).max(1))),
            PaneAction::PageDown => self.work.with_pane(|p| p.step(page)),
            PaneAction::PageUp => self.work.with_pane(|p| p.step(-page)),
            PaneAction::First => self.work.with_pane(|p| p.select_oldest()),
            PaneAction::Last => self.work.with_pane(|p| p.select_newest()),
            PaneAction::OpenThread => match self.work.focused() {
                // Nothing selected: Enter writes, as in GUI Slack.
                Some(p) if p.selected.is_none() || self.work.side == Side::Thread => self.work.insert = true,
                Some(_) => self.work.open_thread(),
                None => {}
            },
            PaneAction::Visual => self.work.with_pane(|p| {
                if p.visual.take().is_none() {
                    if p.selected.is_none() {
                        p.select_newest();
                    }
                    p.visual = p.selected;
                }
            }),
            PaneAction::Escape => self.escape(main_target),
            PaneAction::Copy => self.copy(now),
            PaneAction::Insert => {
                if self.work.focused().is_some() {
                    self.work.insert = true;
                }
            }
            PaneAction::Left => {
                if self.shell.focus == Region::Work && self.work.focus_side(-1) {
                    return;
                }
                if self.shell.list_hidden {
                    self.shell.focus = Region::Rail;
                } else {
                    self.focus_list(main_target);
                }
            }
            PaneAction::Right => {
                if self.shell.focus == Region::Work {
                    self.work.focus_side(1);
                }
            }
            PaneAction::Close => {
                if self.shell.focus != Region::Work {
                    return;
                }
                if let Some(closed) = self.work.close() {
                    self.focus_list(Some(closed));
                }
            }
        }
    }

    /// `Esc` in a pane: one step out (see the table at the top).
    fn escape(&mut self, main: Option<slakio_core::model::Target>) {
        let Some(p) = self.work.focused_mut() else {
            return self.focus_list(None);
        };
        if p.visual.take().is_some() {
            return;
        }
        if p.selected.take().is_some() {
            p.bottom.set(None);
            return;
        }
        if self.work.side == Side::Thread {
            self.work.side = Side::Main;
            return;
        }
        if !self.shell.list_hidden {
            self.focus_list(main);
        }
    }

    /// Copy the selected messages (the VISUAL range, or the selected one) as text: one
    /// message alone as its text, a range as `time  author: text` lines. Sanitised text only.
    fn copy(&mut self, now: Instant) {
        let Some(p) = self.work.focused_mut() else { return };
        let Some((a, b)) = p.range() else { return };
        let one = a == b && p.visual.is_none();
        let items = &p.items[a..=b];
        let text = if one {
            items[0].text.to_string()
        } else {
            items
                .iter()
                .map(|m| format!("{}  {}: {}", crate::time::hm(m.ts), m.author, m.text))
                .collect::<Vec<_>>()
                .join("\n")
        };
        p.visual = None;
        let count = items.len() as u64;
        self.effects.push(Effect::Copy(text));
        self.info(Msg::StatusCopied { count }, now);
    }

    fn composer(&mut self, a: ComposerAction, now: Instant) {
        match a {
            ComposerAction::Send => {
                if self.work.send(&self.model) && self.backend.is_some_and(|c| c.demo) {
                    self.info(Msg::Label(Label::StatusDemoEcho), now);
                }
            }
            ComposerAction::Newline => self.work.with_pane(|p| p.composer.newline()),
            ComposerAction::Leave => self.work.insert = false,
            ComposerAction::DeleteWord => self.work.with_pane(|p| {
                p.composer.delete_word_back();
            }),
            ComposerAction::DeleteLine => self.work.with_pane(|p| {
                p.composer.delete_line_back();
            }),
            ComposerAction::DeleteToEnd => self.work.with_pane(|p| {
                p.composer.delete_to_end();
            }),
        }
    }

    fn command_line(&mut self, a: CommandLineAction, now: Instant) {
        match a {
            CommandLineAction::Open => {
                self.help = None;
                self.cmdline.open();
            }
            CommandLineAction::Cancel => self.cmdline.close(),
            CommandLineAction::Run => self.palette_run(now),
            CommandLineAction::Next => self.palette_step(1),
            CommandLineAction::Prev => self.palette_step(-1),
        }
    }

    /// `:theme <name>`: draw with theme `name` from now on and save it in the config file. A
    /// name the setting does not take changes nothing and says which ones it takes.
    pub fn set_theme(&mut self, name: &str, now: Instant) -> Result<(), Msg> {
        let name = name.trim().to_ascii_lowercase();
        if !theme::NAMES.contains(&name.as_str()) {
            return Err(Msg::ThemeUnknown { name, names: theme::NAMES.join(", ") });
        }
        self.theme = self.look.theme(&name);
        self.theme_setting.clone_from(&name);
        self.effects.push(Effect::Save { key: "theme", value: name.clone() });
        self.info(Msg::ThemeChanged { name }, now);
        Ok(())
    }

    /// `:avatars <initials|off>`: picture people by their initials chip, or not, from now on,
    /// and save it in the config file. Another value changes nothing and says which ones work.
    pub fn set_avatars(&mut self, value: &str, now: Instant) -> Result<(), Msg> {
        let value = value.trim().to_ascii_lowercase();
        if !AVATAR_VALUES.contains(&value.as_str()) {
            return Err(Msg::AvatarsUnknown { name: value, names: AVATAR_VALUES.join(", ") });
        }
        self.settings.avatars = value == "initials";
        self.effects.push(Effect::Save { key: "avatars", value: value.clone() });
        self.info(Msg::AvatarsChanged { name: value }, now);
        Ok(())
    }

    /// Open the keyboard help for context `ctx`.
    fn open_help(&mut self, ctx: Ctx) {
        self.keys.clear();
        self.guide = Guide::default();
        self.help = Some(Help::new(ctx));
    }

    fn help(&mut self, a: HelpAction, now: Instant) {
        if a == HelpAction::Open {
            if self.dialog.is_some() {
                return;
            }
            match self.help {
                Some(_) => self.help = None,
                None => self.open_help(self.screen_context()),
            }
            return;
        }
        let Some(mut h) = self.help.take() else { return };
        let rows = h.rows(&self.keymap, &self.i18n);
        let page = h.height.get().max(1) as isize;
        match a {
            HelpAction::Open | HelpAction::Close => return,
            HelpAction::Next => h.step(1, rows.len()),
            HelpAction::Prev => h.step(-1, rows.len()),
            HelpAction::PageDown => h.step(page, rows.len()),
            HelpAction::PageUp => h.step(-page, rows.len()),
            HelpAction::First => h.cursor = 0,
            HelpAction::Last => h.cursor = rows.len().saturating_sub(1),
            HelpAction::Expand => h.set_open(&rows, true),
            HelpAction::Collapse => h.set_open(&rows, false),
            HelpAction::Search => {
                h.typing = true;
                h.cursor = 0;
            }
            HelpAction::SearchDone => h.typing = false,
            HelpAction::SearchCancel => {
                h.typing = false;
                h.filter.clear();
                h.cursor = 0;
            }
            HelpAction::Run => match rows.get(h.cursor) {
                Some(help::Row::Section { open, .. }) => h.set_open(&rows, !open),
                Some(help::Row::Entry { action, .. }) => {
                    // The help closes, then the key's action runs where the keyboard was.
                    let action = *action;
                    if !matches!(action, Action::Help(_) | Action::Dialog(_)) {
                        self.dispatch(action, now);
                    }
                    return;
                }
                None => {}
            },
        }
        self.help = Some(h);
    }

    fn answer(&mut self, a: DialogAction) {
        let Some(mut d) = self.dialog else { return };
        let yes = match a {
            DialogAction::Yes => true,
            DialogAction::No => false,
            DialogAction::Choose => d.yes,
            DialogAction::Toggle => {
                d.yes = !d.yes;
                if d.question == Question::Icons {
                    // The rail shows the answer that has the focus.
                    self.settings.icons = d.yes;
                }
                self.dialog = Some(d);
                return;
            }
        };
        self.dialog = None;
        match d.question {
            Question::Quit | Question::QuitConfirm => self.quit = yes,
            Question::Icons => {
                self.settings.icons = yes;
                self.effects.push(Effect::Save { key: "icons", value: if yes { "on" } else { "off" }.to_string() });
            }
        }
    }

    /// When the screen changes next by itself, if ever (the loop sleeps until then): a notice
    /// goes away, or the which-key popup shows.
    pub fn deadline(&self) -> Option<Instant> {
        let guide = self.guide.at.filter(|_| !self.guide.shown && !self.keys.pending().is_empty());
        [self.status.deadline(), guide].into_iter().flatten().min()
    }

    /// The deadline passed. `true` when the screen changed.
    pub fn on_tick(&mut self, now: Instant) -> bool {
        let mut changed = self.status.expire(now);
        if let Some(at) = self.guide.at
            && !self.guide.shown
            && now >= at
        {
            self.guide.shown = true;
            changed |= !self.keys.pending().is_empty();
        }
        changed
    }
}

/// The values `:avatars` takes (the config file also keeps `image` for photos, later).
pub const AVATAR_VALUES: &[&str] = &["initials", "off"];

/// The value of `:avatars <value>` (also `:set avatars=<value>`).
pub fn avatars_arg(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let (word, rest) = line.split_once(char::is_whitespace)?;
    match word {
        "avatars" => Some(rest.trim()),
        "set" => rest.trim().strip_prefix("avatars=").map(str::trim),
        _ => None,
    }
}

/// The theme name of `:theme <name>` (also `:colorscheme`, `:colo` as in vim, and
/// `:set theme=<name>`).
pub fn theme_arg(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let (word, rest) = line.split_once(char::is_whitespace)?;
    match word {
        "theme" | "colorscheme" | "colo" => Some(rest.trim()),
        "set" => rest.trim().strip_prefix("theme=").map(str::trim),
        _ => None,
    }
}
