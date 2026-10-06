//! The application state. `App` is a thin router: each part of the state lives in a sub-state
//! that owns its data and its update ([`cmdline::CommandLine`] with its [`palette`],
//! [`status::Status`],
//! [`shell::Shell`], [`work::Work`] with its [`pane::Pane`] views, [`timelines`] and [`drafts`],
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
//! | the main pane | the list, on its conversation | closes it, and its tab with its last pane: the tab shown next, else the list, on its conversation |
//! | the view switcher | the list | |
//! | the list | nothing | |
//!
//! The view switcher (the list panel's first row) is reached from anywhere outside text with
//! `Ctrl+R` or `Space r` ([`nav`]), and is a stop of the `Tab` round (before the list). Tabs ([`tabs`]) keep their own panes; the mouse is
//! [`mouse`]'s.

pub mod cmdline;
pub mod composer;
pub mod dialog;
pub(crate) mod drafts;
mod focus;
pub mod help;
mod layout;
pub mod model;
mod mouse;
mod nav;
mod overlay;
pub mod palette;
pub mod pane;
pub mod query;
pub(crate) mod requests;
pub mod shell;
pub mod status;
mod tabs;
pub(crate) mod timelines;
pub(crate) mod work;

use crate::action::{Action, AppAction, CommandLineAction, ComposerAction, DialogAction, PaneAction};
use crate::input::hangul;
use crate::keymap::{Ctx, KeyChord, KeyState, Keymap, Resolved};
use crate::screen;
use crate::theme::{Look, Theme};
use composer::Composer;
use dialog::{Dialog, Question};
use help::Help;
use model::Model;
pub(crate) use overlay::Layer;
pub use query::{Focus, Overlay, PaneHandle, PaneKind, PaneRef};
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Position, Rect};
use shell::Shell;
use slakio_core::backend::{Capabilities, Command, Envelope, Event as BackendEvent, Generation};
use slakio_core::i18n::{I18n, Label, Lang, Msg};
use status::{Level, Status};
use std::time::{Duration, Instant};
pub use tabs::Unread;
use work::Work;

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
    /// People are pictured by an initials chip (`avatars = "initials"`, the default).
    pub avatars: bool,
    /// Messages in compact columns instead of the comfortable layout (`density`).
    pub compact: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { icons: false, avatars: true, compact: false }
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
    pub(crate) shell: Shell,
    /// The region with the keyboard ([`focus`]); in the work area, its active pane has it.
    region: shell::Region,
    pub(crate) work: Work,
    pub model: Model,
    /// The keyboard help, while open.
    pub(crate) help: Option<Help>,
    /// A question, while asked.
    pub(crate) dialog: Option<Dialog>,
    /// What the backend can do; `None` without one.
    pub backend: Option<Capabilities>,
    /// The generation of the last boot request; older answers are dropped.
    boot: Generation,
    effects: Vec<Effect>,
    /// The last click (for a double click): when, and where.
    last_click: Option<(Instant, Position)>,
    /// The tab being dragged along the tab bar (its index now).
    tab_drag: Option<usize>,
    /// The workspace switcher's cursor, while it is open.
    pub(crate) switcher: Option<usize>,
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
            region: shell::Region::List,
            work: Work::default(),
            model: Model::default(),
            help: None,
            dialog: None,
            backend: None,
            boot: Generation::default(),
            effects: Vec::new(),
            last_click: None,
            tab_drag: None,
            switcher: None,
            size: Rect::default(),
            quit: false,
        }
    }

    /// Use a backend that can do `caps`: asks it for the workspaces.
    pub fn connect(&mut self, caps: Capabilities) {
        self.backend = Some(caps);
        self.boot = self.work.requests.ask(Command::Boot);
    }

    /// Ask once whether the terminal shows Nerd Font icons; the view switcher previews the answer that
    /// has the focus, and the answer is saved ([`Effect::Save`]).
    pub fn ask_icons(&mut self) {
        self.dialog = Some(Dialog::new(Question::Icons));
        self.settings.icons = false;
    }

    /// The requests for the backend queued since the last call.
    pub fn take_commands(&mut self) -> Vec<(Generation, Command)> {
        self.work.take_requests()
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

    /// Where the keyboard is: the top popup, else the screen.
    pub fn key_context(&self) -> Ctx {
        match self.overlay() {
            Some(Overlay::Dialog(_)) => Ctx::Dialog,
            Some(Overlay::Help) if self.help.as_ref().is_some_and(|h| h.typing) => Ctx::HelpFilter,
            Some(Overlay::Help) => Ctx::Help,
            Some(Overlay::Palette) => Ctx::CommandLine,
            Some(Overlay::Switcher) => Ctx::Switcher,
            None => self.region_context(),
        }
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
        match self.focus() {
            Focus::ViewSwitcher => Ctx::ViewSwitcher,
            Focus::List => Ctx::List,
            Focus::Pane(_) if self.work.insert() => Ctx::ComposerInsert,
            Focus::Pane(_) if self.work.focused().is_some_and(|p| p.visual.is_some()) => Ctx::PaneVisual,
            Focus::Pane(_) | Focus::Work => Ctx::PaneNormal,
        }
    }

    /// Rows the list panel shows (or would show, while it is hidden).
    fn list_height(&self) -> usize {
        let a = self.areas();
        let panel = a.list.unwrap_or(a.work);
        usize::from(screen::list_parts(panel).rows.height).max(1)
    }

    /// Rows of messages the focused pane shows.
    fn pane_height(&self) -> usize {
        let frame = self.frame();
        self.work.active().and_then(|id| frame.pane(id)).map_or(1, |p| usize::from(p.parts.messages.height)).max(1)
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
                        if let Some(c) = self.work.draft_mut() {
                            c.insert(&text);
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
                let Some(c) = self.work.draft_mut() else { return };
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

    /// Route `action` to the sub-state that owns it.
    pub fn dispatch(&mut self, action: Action, now: Instant) {
        match action {
            Action::App(a) => self.app(a, now),
            Action::CommandLine(a) => self.command_line(a, now),
            Action::Shell(a) => self.shell(a, now),
            Action::Pane(a) => self.pane(a, now),
            Action::Tab(a) => self.tab(a, now),
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
            AppAction::ToggleAvatars | AppAction::ToggleDensity | AppAction::ToggleIcons => self.toggle(a, now),
            AppAction::ChooseWorkspace => self.open_switcher(),
        }
    }

    /// Ask before quitting: about the text not sent when a composer holds some, else only
    /// whether to quit (for a click, which is easy to make by mistake).
    fn confirm_quit(&mut self) {
        self.help = None;
        let q = if self.work.unsent() { Question::Quit } else { Question::QuitConfirm };
        self.dialog = Some(Dialog::new(q));
    }

    /// Open a conversation in the main pane; `focus`: give it the keyboard, as GUI Slack does
    /// (the next `j`/`k` moves through its messages).
    fn open(&mut self, target: slakio_core::model::Target, focus: bool) {
        let Some(id) = self.work.open(target) else { return };
        self.work.activate(id);
        if focus {
            self.set_focus(Focus::on(id));
        }
    }

    /// The thread of the selected message in the thread panel, with the keyboard.
    fn open_thread(&mut self) {
        if let Some(id) = self.work.open_thread() {
            self.set_focus(Focus::on(id));
        }
    }

    fn pane(&mut self, a: PaneAction, now: Instant) {
        if self.backend.is_none() {
            return;
        }
        let page = self.pane_height() as isize;
        let main_target = self.work.home().map(|p| p.target.clone());
        match a {
            PaneAction::Back | PaneAction::Forward => {
                let moved = if a == PaneAction::Back { self.work.back() } else { self.work.forward() };
                if let Some(id) = moved {
                    self.set_focus(Focus::on(id));
                } else {
                    let l = if a == PaneAction::Back { Label::StatusNoEarlier } else { Label::StatusNoLater };
                    self.info(Msg::Label(l), now);
                }
            }
            PaneAction::Next => self.work.step(1),
            PaneAction::Prev => self.work.step(-1),
            PaneAction::HalfDown => self.work.step((page / 2).max(1)),
            PaneAction::HalfUp => self.work.step(-(page / 2).max(1)),
            PaneAction::PageDown => self.work.step(page),
            PaneAction::PageUp => self.work.step(-page),
            PaneAction::First => self.work.with_pane(|p, tl| p.select_oldest(tl)),
            PaneAction::Last => self.work.with_pane(|p, tl| p.select_newest(tl)),
            PaneAction::OpenThread => match self.work.focused() {
                // Nothing selected: Enter writes, as in GUI Slack.
                Some(p) if p.selected.is_none() || p.is_thread() => {
                    self.work.set_insert(true);
                }
                Some(_) => self.open_thread(),
                None => {}
            },
            PaneAction::Visual => self.work.with_pane(|p, tl| {
                if p.visual.take().is_none() {
                    if p.selected.is_none() {
                        p.select_newest(tl);
                    }
                    p.visual = p.selected;
                }
            }),
            PaneAction::Escape => self.escape(main_target),
            PaneAction::Copy => self.copy(now),
            PaneAction::Insert => {
                if self.work.focused().is_some() {
                    self.work.set_insert(true);
                }
            }
            PaneAction::Left => {
                if self.focus().is_pane()
                    && let Some(id) = self.work.beside(-1)
                {
                    return self.set_focus(Focus::on(id));
                }
                if !self.shell.list_hidden {
                    self.focus_list(main_target);
                }
            }
            PaneAction::Right => {
                if self.focus().is_pane()
                    && let Some(id) = self.work.beside(1)
                {
                    self.set_focus(Focus::on(id));
                }
            }
            PaneAction::Close => self.close_pane(now),
        }
    }

    /// Copy the selected messages (the VISUAL range, or the selected one) as text: one
    /// message alone as its text, a range as `time  author: text` lines. Sanitised text only.
    fn copy(&mut self, now: Instant) {
        let Some(p) = self.work.focused() else { return };
        let Some((a, b)) = p.range(self.work.timeline(p)) else { return };
        let one = a == b && p.visual.is_none();
        let items = &self.work.timeline(p).items[a..=b];
        let text = if one {
            items[0].text.to_string()
        } else {
            items
                .iter()
                .map(|m| format!("{}  {}: {}", crate::time::hm(m.ts), m.author, m.text))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let count = items.len() as u64;
        if let Some(p) = self.work.focused_mut() {
            p.visual = None;
        }
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
            ComposerAction::Newline => self.work.with_draft(Composer::newline),
            ComposerAction::Leave => self.work.set_insert(false),
            ComposerAction::DeleteWord => self.work.with_draft(|c| {
                c.delete_word_back();
            }),
            ComposerAction::DeleteLine => self.work.with_draft(|c| {
                c.delete_line_back();
            }),
            ComposerAction::DeleteToEnd => self.work.with_draft(|c| {
                c.delete_to_end();
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

    fn answer(&mut self, a: DialogAction) {
        let Some(mut d) = self.dialog else { return };
        let yes = match a {
            DialogAction::Yes => true,
            DialogAction::No => false,
            DialogAction::Choose => d.yes,
            DialogAction::Toggle => {
                d.yes = !d.yes;
                if d.question == Question::Icons {
                    // The view switcher shows the answer that has the focus.
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
