//! Context key map: one table of default bindings ([`DEFAULTS`]) decides what a key does in the
//! current [`Ctx`]. A few keys work everywhere, also while typing ([`Ctx::Global`]: quit, help,
//! the command palette); then keys resolve in the current context first, then in its parents. A
//! text input context has no parent, so every key no binding of its own claims is typed; a
//! modal context (the help, a question) has none either, so only its own keys work there.
//!
//! ```text
//! global           (always first: Ctrl+Q, F1, Ctrl+P)
//! root
//! └─ shell            (the non-text regions: leader `Space`, focus keys, back/forward)
//!    ├─ nav          (the top bar)
//!    ├─ list
//!    └─ pane.normal   (the work area in Normal mode)
//!       └─ pane.visual   (a VISUAL range of messages)
//! cmdline          [text]  (the `:` command line)
//! composer.insert  [text]  (a pane's composer in Insert mode)
//! help             [modal] (the keyboard help)
//! help.filter      [text]  (typing the help's search)
//! dialog           [modal] (a question with two answers)
//! ```
//!
//! * [`keys`] — key chords and their notation (`ctrl+e`, `esc`, `space w h`).
//! * [`check`] — the key conflict checker; the tests run it on the defaults.
//! * [`doc`] — renders `docs/keybindings.md` from this table and the action registry.
//! * [`hints`] — the hint line's entries for where the keyboard is.
//! * [`guide`] — what may follow an unfinished sequence (the which-key popup).
//!
//! Key sequences (`g g`, `Space w h`) are collected by [`Keymap::feed`]. User key bindings
//! (`[keymap.<ctx>]`) build on this table later and go through the same checker.

pub mod check;
pub mod doc;
pub mod guide;
pub mod hints;
pub mod keys;

pub use check::{Conflict, ConflictKind, check};
pub use keys::{KeyChord, KeyError, parse_keys};

use crate::action::{
    Action, AppAction, CommandLineAction, ComposerAction, DialogAction, HelpAction, PaneAction, ShellAction, TabAction,
};
use crate::app::shell::View;
use slakio_core::i18n::Label;

/// A key context: where the keyboard is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ctx {
    /// Looked up before any other context, also in text inputs and modal ones.
    Global,
    /// Applies everywhere a context of its own does not take the key (text inputs and modal
    /// contexts excepted).
    Root,
    /// The non-text regions of the main screen.
    Shell,
    /// The top bar: the workspace and the views.
    Nav,
    /// The list panel: sections, channels, DMs.
    List,
    /// The work area in Normal mode.
    PaneNormal,
    /// A VISUAL range of messages.
    PaneVisual,
    /// The `:` command line (text input).
    CommandLine,
    /// A pane's composer in Insert mode (text input).
    ComposerInsert,
    /// The keyboard help (modal).
    Help,
    /// Typing the keyboard help's search (text input).
    HelpFilter,
    /// A question with two answers (modal).
    Dialog,
    /// The workspace switcher (modal).
    Switcher,
}

impl Ctx {
    /// Every context, in the order the docs list them.
    pub const ALL: &'static [Ctx] = &[
        Ctx::Global,
        Ctx::Root,
        Ctx::Shell,
        Ctx::Nav,
        Ctx::List,
        Ctx::PaneNormal,
        Ctx::PaneVisual,
        Ctx::CommandLine,
        Ctx::ComposerInsert,
        Ctx::Help,
        Ctx::HelpFilter,
        Ctx::Dialog,
        Ctx::Switcher,
    ];

    /// The id config files and the docs use.
    pub fn id(self) -> &'static str {
        match self {
            Ctx::Global => "global",
            Ctx::Root => "root",
            Ctx::Shell => "shell",
            Ctx::Nav => "nav",
            Ctx::List => "list",
            Ctx::PaneNormal => "pane.normal",
            Ctx::PaneVisual => "pane.visual",
            Ctx::CommandLine => "cmdline",
            Ctx::ComposerInsert => "composer.insert",
            Ctx::Help => "help",
            Ctx::HelpFilter => "help.filter",
            Ctx::Dialog => "dialog",
            Ctx::Switcher => "switcher",
        }
    }

    pub fn label(self) -> Label {
        match self {
            Ctx::Global => Label::CtxGlobal,
            Ctx::Root => Label::CtxRoot,
            Ctx::Shell => Label::CtxShell,
            Ctx::Nav => Label::CtxNav,
            Ctx::List => Label::CtxList,
            Ctx::PaneNormal => Label::CtxPaneNormal,
            Ctx::PaneVisual => Label::CtxPaneVisual,
            Ctx::CommandLine => Label::CtxCmdline,
            Ctx::ComposerInsert => Label::CtxComposerInsert,
            Ctx::Help => Label::CtxHelp,
            Ctx::HelpFilter => Label::CtxHelpFilter,
            Ctx::Dialog => Label::CtxDialog,
            Ctx::Switcher => Label::CtxSwitcher,
        }
    }

    /// The context a key not bound here is looked up in next. A text input or a modal context
    /// has none (and [`Ctx::Global`] is looked up before any).
    pub fn parent(self) -> Option<Ctx> {
        match self {
            Ctx::Global
            | Ctx::Root
            | Ctx::CommandLine
            | Ctx::ComposerInsert
            | Ctx::Help
            | Ctx::HelpFilter
            | Ctx::Dialog
            | Ctx::Switcher => None,
            Ctx::Shell => Some(Ctx::Root),
            Ctx::Nav | Ctx::List | Ctx::PaneNormal => Some(Ctx::Shell),
            Ctx::PaneVisual => Some(Ctx::PaneNormal),
        }
    }

    /// This context and its parents, innermost first.
    pub fn chain(self) -> Vec<Ctx> {
        std::iter::successors(Some(self), |c| c.parent()).collect()
    }

    /// Characters typed here are text, not commands.
    pub fn is_text_input(self) -> bool {
        matches!(self, Ctx::CommandLine | Ctx::ComposerInsert | Ctx::HelpFilter)
    }

    /// Only its own keys (and the global ones) work here.
    pub fn is_modal(self) -> bool {
        matches!(self, Ctx::Help | Ctx::Dialog | Ctx::Switcher)
    }
}

/// The leader key of the non-text contexts: an unfinished `Space …` sequence is dropped.
pub const LEADER: &str = "space";

/// A default binding: in `ctx`, `keys` (config notation) runs `action`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub ctx: Ctx,
    pub keys: &'static str,
    pub action: Action,
    /// Bound only with the kitty keyboard protocol on (without it the terminal sends the key as
    /// another one: `Ctrl+I` as `Tab`).
    pub kitty: bool,
}

const fn bind(ctx: Ctx, keys: &'static str, action: Action) -> Binding {
    Binding { ctx, keys, action, kitty: false }
}

const fn app(ctx: Ctx, keys: &'static str, action: AppAction) -> Binding {
    bind(ctx, keys, Action::App(action))
}

const fn shell(ctx: Ctx, keys: &'static str, action: ShellAction) -> Binding {
    bind(ctx, keys, Action::Shell(action))
}

const fn pane(ctx: Ctx, keys: &'static str, action: PaneAction) -> Binding {
    bind(ctx, keys, Action::Pane(action))
}

const fn tab(ctx: Ctx, keys: &'static str, action: TabAction) -> Binding {
    bind(ctx, keys, Action::Tab(action))
}

const fn composer(keys: &'static str, action: ComposerAction) -> Binding {
    bind(Ctx::ComposerInsert, keys, Action::Composer(action))
}

const fn help(ctx: Ctx, keys: &'static str, action: HelpAction) -> Binding {
    bind(ctx, keys, Action::Help(action))
}

const fn dialog(keys: &'static str, action: DialogAction) -> Binding {
    bind(Ctx::Dialog, keys, Action::Dialog(action))
}

const fn cmdline(ctx: Ctx, keys: &'static str, action: CommandLineAction) -> Binding {
    bind(ctx, keys, Action::CommandLine(action))
}

/// The default bindings. Where an action has several keys, the first that works without the
/// kitty keyboard protocol is the one hints show.
pub const DEFAULTS: &[Binding] = &[
    // Everywhere, also while typing: quit (asks first when a message is not sent), help and the
    // command palette. `q` alone never quits: in a chat it is too easily typed in Normal mode.
    app(Ctx::Global, "ctrl+q", AppAction::Quit),
    help(Ctx::Global, "f1", HelpAction::Open),
    app(Ctx::Global, "ctrl+p", AppAction::Palette),
    cmdline(Ctx::Root, ":", CommandLineAction::Open),
    help(Ctx::Root, "?", HelpAction::Open),
    app(Ctx::Root, "ctrl+c", AppAction::Interrupt),
    // Between the panels. `Tab` goes round the top bar, list, main pane and thread panel.
    // `Ctrl+R` and `Space r` go straight to the top bar and
    // `Ctrl+R` back. `Ctrl+H` needs the kitty keyboard protocol on terminals that send
    // `Backspace` as `^H`; `Space w h` works everywhere.
    shell(Ctx::Shell, "tab", ShellAction::FocusNext),
    shell(Ctx::Shell, "f6", ShellAction::FocusNext),
    shell(Ctx::Shell, "shift+tab", ShellAction::FocusPrev),
    shell(Ctx::Shell, "shift+f6", ShellAction::FocusPrev),
    shell(Ctx::Shell, "ctrl+r", ShellAction::FocusNav),
    shell(Ctx::Shell, "space r", ShellAction::FocusNav),
    shell(Ctx::Shell, "ctrl+h", ShellAction::FocusLeft),
    shell(Ctx::Shell, "ctrl+j", ShellAction::FocusDown),
    shell(Ctx::Shell, "ctrl+k", ShellAction::FocusUp),
    shell(Ctx::Shell, "ctrl+l", ShellAction::FocusRight),
    shell(Ctx::Shell, "space w h", ShellAction::FocusLeft),
    shell(Ctx::Shell, "space w j", ShellAction::FocusDown),
    shell(Ctx::Shell, "space w k", ShellAction::FocusUp),
    shell(Ctx::Shell, "space w l", ShellAction::FocusRight),
    pane(Ctx::Shell, "space w c", PaneAction::Close),
    // The top bar.s views, by key.
    shell(Ctx::Shell, "space h", ShellAction::Show(View::Home)),
    shell(Ctx::Shell, "space d", ShellAction::Show(View::Dms)),
    shell(Ctx::Shell, "space a", ShellAction::Show(View::Activity)),
    shell(Ctx::Shell, "space f", ShellAction::Show(View::Files)),
    shell(Ctx::Shell, "space l", ShellAction::Show(View::Later)),
    shell(Ctx::Shell, "space e", ShellAction::ToggleList),
    app(Ctx::Shell, "space W", AppAction::ChooseWorkspace),
    // Back and forward through what the work area showed. `Ctrl+I` is `Tab` without the kitty
    // keyboard protocol, so it is bound only with it; `Alt` needs Option-as-Alt on macOS;
    // `Space [` and `Space ]` work everywhere.
    pane(Ctx::Shell, "ctrl+o", PaneAction::Back),
    pane(Ctx::Shell, "alt+left", PaneAction::Back),
    pane(Ctx::Shell, "space [", PaneAction::Back),
    Binding { kitty: true, ..pane(Ctx::Shell, "ctrl+i", PaneAction::Forward) },
    pane(Ctx::Shell, "alt+right", PaneAction::Forward),
    pane(Ctx::Shell, "space ]", PaneAction::Forward),
    // Tabs. `Alt+1..9` needs Option-as-Alt on macOS; `Space 1..9` works everywhere.
    tab(Ctx::Shell, "g t", TabAction::Next),
    tab(Ctx::Shell, "g T", TabAction::Prev),
    tab(Ctx::Shell, "ctrl+pagedown", TabAction::Next),
    tab(Ctx::Shell, "ctrl+pageup", TabAction::Prev),
    tab(Ctx::Shell, "space 1", TabAction::Go(1)),
    tab(Ctx::Shell, "space 2", TabAction::Go(2)),
    tab(Ctx::Shell, "space 3", TabAction::Go(3)),
    tab(Ctx::Shell, "space 4", TabAction::Go(4)),
    tab(Ctx::Shell, "space 5", TabAction::Go(5)),
    tab(Ctx::Shell, "space 6", TabAction::Go(6)),
    tab(Ctx::Shell, "space 7", TabAction::Go(7)),
    tab(Ctx::Shell, "space 8", TabAction::Go(8)),
    tab(Ctx::Shell, "space 9", TabAction::Go(9)),
    tab(Ctx::Shell, "alt+1", TabAction::Go(1)),
    tab(Ctx::Shell, "alt+2", TabAction::Go(2)),
    tab(Ctx::Shell, "alt+3", TabAction::Go(3)),
    tab(Ctx::Shell, "alt+4", TabAction::Go(4)),
    tab(Ctx::Shell, "alt+5", TabAction::Go(5)),
    tab(Ctx::Shell, "alt+6", TabAction::Go(6)),
    tab(Ctx::Shell, "alt+7", TabAction::Go(7)),
    tab(Ctx::Shell, "alt+8", TabAction::Go(8)),
    tab(Ctx::Shell, "alt+9", TabAction::Go(9)),
    tab(Ctx::Shell, "space t n", TabAction::Open),
    tab(Ctx::Shell, "space t c", TabAction::Close),
    tab(Ctx::Shell, "space t u", TabAction::Reopen),
    tab(Ctx::Shell, "space t r", TabAction::Rename),
    tab(Ctx::Shell, "space t h", TabAction::MoveLeft),
    tab(Ctx::Shell, "space t l", TabAction::MoveRight),
    help(Ctx::Shell, "space ?", HelpAction::Open),
    cmdline(Ctx::Shell, "space /", CommandLineAction::Open),
    app(Ctx::Shell, "space q", AppAction::Quit),
    // The top bar runs left to right; down (or Esc) goes back to the panels under it.
    shell(Ctx::Nav, "l", ShellAction::NavNext),
    shell(Ctx::Nav, "right", ShellAction::NavNext),
    shell(Ctx::Nav, "h", ShellAction::NavPrev),
    shell(Ctx::Nav, "left", ShellAction::NavPrev),
    shell(Ctx::Nav, "g g", ShellAction::NavFirst),
    shell(Ctx::Nav, "home", ShellAction::NavFirst),
    shell(Ctx::Nav, "G", ShellAction::NavLast),
    shell(Ctx::Nav, "end", ShellAction::NavLast),
    shell(Ctx::Nav, "enter", ShellAction::NavSelect),
    shell(Ctx::Nav, "esc", ShellAction::NavLeave),
    shell(Ctx::Nav, "j", ShellAction::NavLeave),
    shell(Ctx::Nav, "down", ShellAction::NavLeave),
    // The workspace switcher (a popup).
    shell(Ctx::Switcher, "j", ShellAction::SwitcherNext),
    shell(Ctx::Switcher, "down", ShellAction::SwitcherNext),
    shell(Ctx::Switcher, "k", ShellAction::SwitcherPrev),
    shell(Ctx::Switcher, "up", ShellAction::SwitcherPrev),
    shell(Ctx::Switcher, "enter", ShellAction::SwitcherChoose),
    shell(Ctx::Switcher, "esc", ShellAction::SwitcherClose),
    shell(Ctx::Switcher, "q", ShellAction::SwitcherClose),
    shell(Ctx::Switcher, "ctrl+c", ShellAction::SwitcherClose),
    shell(Ctx::List, "j", ShellAction::ListNext),
    shell(Ctx::List, "down", ShellAction::ListNext),
    shell(Ctx::List, "k", ShellAction::ListPrev),
    shell(Ctx::List, "up", ShellAction::ListPrev),
    shell(Ctx::List, "g g", ShellAction::ListFirst),
    shell(Ctx::List, "home", ShellAction::ListFirst),
    shell(Ctx::List, "G", ShellAction::ListLast),
    shell(Ctx::List, "end", ShellAction::ListLast),
    shell(Ctx::List, "ctrl+d", ShellAction::ListHalfDown),
    shell(Ctx::List, "ctrl+u", ShellAction::ListHalfUp),
    shell(Ctx::List, "pagedown", ShellAction::ListPageDown),
    shell(Ctx::List, "pageup", ShellAction::ListPageUp),
    shell(Ctx::List, "enter", ShellAction::ListOpen),
    shell(Ctx::List, "l", ShellAction::ListPeek),
    shell(Ctx::List, "right", ShellAction::ListPeek),
    shell(Ctx::List, "h", ShellAction::ListLeft),
    shell(Ctx::List, "left", ShellAction::ListLeft),
    shell(Ctx::List, "{", ShellAction::ListSectionPrev),
    shell(Ctx::List, "}", ShellAction::ListSectionNext),
    tab(Ctx::List, "t", TabAction::Open),
    pane(Ctx::PaneNormal, "j", PaneAction::Next),
    pane(Ctx::PaneNormal, "down", PaneAction::Next),
    pane(Ctx::PaneNormal, "k", PaneAction::Prev),
    pane(Ctx::PaneNormal, "up", PaneAction::Prev),
    pane(Ctx::PaneNormal, "g g", PaneAction::First),
    pane(Ctx::PaneNormal, "home", PaneAction::First),
    pane(Ctx::PaneNormal, "G", PaneAction::Last),
    pane(Ctx::PaneNormal, "end", PaneAction::Last),
    pane(Ctx::PaneNormal, "ctrl+d", PaneAction::HalfDown),
    pane(Ctx::PaneNormal, "ctrl+u", PaneAction::HalfUp),
    pane(Ctx::PaneNormal, "pagedown", PaneAction::PageDown),
    pane(Ctx::PaneNormal, "pageup", PaneAction::PageUp),
    pane(Ctx::PaneNormal, "enter", PaneAction::OpenThread),
    pane(Ctx::PaneNormal, "i", PaneAction::Insert),
    pane(Ctx::PaneNormal, "a", PaneAction::Insert),
    pane(Ctx::PaneNormal, "y", PaneAction::Copy),
    pane(Ctx::PaneNormal, "V", PaneAction::Visual),
    tab(Ctx::PaneNormal, "t", TabAction::Open),
    // VISUAL, the selection, the thread panel and the main pane are left one `Esc` at a time;
    // nothing is closed by it (only `Ctrl+W` closes).
    pane(Ctx::PaneNormal, "esc", PaneAction::Escape),
    pane(Ctx::PaneNormal, "h", PaneAction::Left),
    pane(Ctx::PaneNormal, "left", PaneAction::Left),
    pane(Ctx::PaneNormal, "l", PaneAction::Right),
    pane(Ctx::PaneNormal, "right", PaneAction::Right),
    pane(Ctx::PaneNormal, "ctrl+w", PaneAction::Close),
    // `Shift+Enter` needs the kitty keyboard protocol (else it arrives as `Enter`); `Alt+Enter`
    // and `Ctrl+J` work everywhere.
    composer("enter", ComposerAction::Send),
    composer("alt+enter", ComposerAction::Newline),
    composer("ctrl+j", ComposerAction::Newline),
    composer("shift+enter", ComposerAction::Newline),
    composer("esc", ComposerAction::Leave),
    composer("ctrl+c", ComposerAction::Leave),
    composer("ctrl+w", ComposerAction::DeleteWord),
    composer("ctrl+u", ComposerAction::DeleteLine),
    composer("ctrl+k", ComposerAction::DeleteToEnd),
    cmdline(Ctx::CommandLine, "enter", CommandLineAction::Run),
    cmdline(Ctx::CommandLine, "esc", CommandLineAction::Cancel),
    cmdline(Ctx::CommandLine, "ctrl+c", CommandLineAction::Cancel),
    // The palette's list (`Ctrl+P`, global, closes it).
    cmdline(Ctx::CommandLine, "down", CommandLineAction::Next),
    cmdline(Ctx::CommandLine, "tab", CommandLineAction::Next),
    cmdline(Ctx::CommandLine, "ctrl+n", CommandLineAction::Next),
    cmdline(Ctx::CommandLine, "up", CommandLineAction::Prev),
    cmdline(Ctx::CommandLine, "shift+tab", CommandLineAction::Prev),
    help(Ctx::Help, "j", HelpAction::Next),
    help(Ctx::Help, "down", HelpAction::Next),
    help(Ctx::Help, "k", HelpAction::Prev),
    help(Ctx::Help, "up", HelpAction::Prev),
    help(Ctx::Help, "pagedown", HelpAction::PageDown),
    help(Ctx::Help, "ctrl+d", HelpAction::PageDown),
    help(Ctx::Help, "pageup", HelpAction::PageUp),
    help(Ctx::Help, "ctrl+u", HelpAction::PageUp),
    help(Ctx::Help, "g g", HelpAction::First),
    help(Ctx::Help, "home", HelpAction::First),
    help(Ctx::Help, "G", HelpAction::Last),
    help(Ctx::Help, "end", HelpAction::Last),
    help(Ctx::Help, "enter", HelpAction::Run),
    help(Ctx::Help, "l", HelpAction::Expand),
    help(Ctx::Help, "right", HelpAction::Expand),
    help(Ctx::Help, "h", HelpAction::Collapse),
    help(Ctx::Help, "left", HelpAction::Collapse),
    help(Ctx::Help, "/", HelpAction::Search),
    help(Ctx::Help, "esc", HelpAction::Close),
    help(Ctx::Help, "q", HelpAction::Close),
    help(Ctx::Help, "?", HelpAction::Close),
    help(Ctx::Help, "ctrl+c", HelpAction::Close),
    help(Ctx::HelpFilter, "enter", HelpAction::SearchDone),
    help(Ctx::HelpFilter, "down", HelpAction::SearchDone),
    help(Ctx::HelpFilter, "esc", HelpAction::SearchCancel),
    help(Ctx::HelpFilter, "ctrl+c", HelpAction::SearchCancel),
    // A question: the safe answer has the focus, so `Enter` alone never does the risky thing.
    dialog("y", DialogAction::Yes),
    dialog("n", DialogAction::No),
    dialog("esc", DialogAction::No),
    dialog("ctrl+c", DialogAction::No),
    dialog("enter", DialogAction::Choose),
    dialog("tab", DialogAction::Toggle),
    dialog("left", DialogAction::Toggle),
    dialog("right", DialogAction::Toggle),
    dialog("h", DialogAction::Toggle),
    dialog("l", DialogAction::Toggle),
];

/// A binding in effect, parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bound {
    pub ctx: Ctx,
    pub keys: Vec<KeyChord>,
    pub action: Action,
}

/// Keys typed so far of an unfinished sequence, and the context they were typed in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyState {
    pending: Vec<KeyChord>,
    ctx: Option<Ctx>,
}

impl KeyState {
    pub fn pending(&self) -> &[KeyChord] {
        &self.pending
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.ctx = None;
    }

    /// The context the unfinished sequence was typed in.
    pub fn ctx(&self) -> Option<Ctx> {
        self.ctx
    }

    /// Drop the last key of the unfinished sequence (`Backspace` while the which-key popup
    /// shows). `true` when there was one.
    pub fn back(&mut self) -> bool {
        let popped = self.pending.pop().is_some();
        if self.pending.is_empty() {
            self.ctx = None;
        }
        popped
    }
}

/// What a key did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved {
    Action(Action),
    /// A prefix of a longer binding: wait for the next key.
    Pending,
    /// No binding: the keys go to what has the focus (typed in a text input, else dropped).
    Unbound(Vec<KeyChord>),
}

/// The bindings in effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keymap {
    bound: Vec<Bound>,
}

/// Without the kitty keyboard protocol (what a terminal is assumed to do until it says
/// otherwise).
impl Default for Keymap {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Keymap {
    /// The default bindings; `enhanced`: the kitty keyboard protocol is on.
    pub fn new(enhanced: bool) -> Self {
        let bound = DEFAULTS
            .iter()
            .filter(|b| enhanced || !b.kitty)
            .map(|b| Bound {
                ctx: b.ctx,
                keys: parse_keys(b.keys).expect("default key notation is valid (tested)"),
                action: b.action,
            })
            .collect();
        Self { bound }
    }

    pub fn from_bound(bound: Vec<Bound>) -> Self {
        Self { bound }
    }

    pub fn all(&self) -> &[Bound] {
        &self.bound
    }

    /// The bindings of `ctx` itself, in table order: (keys, action).
    pub fn bindings(&self, ctx: Ctx) -> impl Iterator<Item = (&[KeyChord], Action)> {
        self.bound.iter().filter(move |b| b.ctx == ctx).map(|b| (b.keys.as_slice(), b.action))
    }

    /// Resolve key `k` typed in `ctx`, continuing the sequence in `st` (a sequence started in
    /// another context is dropped).
    pub fn feed(&self, st: &mut KeyState, ctx: Ctx, k: KeyChord) -> Resolved {
        if let Some(b) = self.bound.iter().find(|b| b.ctx == Ctx::Global && b.keys == [k]) {
            st.clear();
            return Resolved::Action(b.action);
        }
        if st.ctx != Some(ctx) {
            st.clear();
        }
        let mut seq = std::mem::take(&mut st.pending);
        seq.push(k);
        let mut longer = false;
        for c in ctx.chain() {
            for b in self.bound.iter().filter(|b| b.ctx == c) {
                if b.keys == seq {
                    st.clear();
                    return Resolved::Action(b.action);
                }
                longer |= b.keys.len() > seq.len() && b.keys.starts_with(&seq);
            }
        }
        if longer {
            st.pending = seq;
            st.ctx = Some(ctx);
            return Resolved::Pending;
        }
        st.clear();
        Resolved::Unbound(seq)
    }

    /// The key sequences that run `action` from `ctx` (innermost first, the global keys last),
    /// for hints.
    pub fn keys_for(&self, action: Action, ctx: Ctx) -> Vec<Vec<KeyChord>> {
        ctx.chain()
            .into_iter()
            .chain(std::iter::once(Ctx::Global))
            .flat_map(|c| self.bound.iter().filter(move |b| b.ctx == c && b.action == action))
            .map(|b| b.keys.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests;
