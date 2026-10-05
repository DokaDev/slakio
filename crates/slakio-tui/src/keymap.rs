//! Context key map: one table of default bindings ([`DEFAULTS`]) decides what a key does in the
//! current [`Ctx`]. Keys resolve in the current context first, then in its parents; a text
//! input context has no parent, so every key no binding of its own claims is typed.
//!
//! ```text
//! root
//! └─ shell            (the non-text regions: leader `Space`, focus keys)
//!    ├─ rail
//!    ├─ list
//!    └─ pane.normal   (the work area in Normal mode)
//! cmdline  [text]     (the `:` command line)
//! ```
//!
//! * [`keys`] — key chords and their notation (`ctrl+e`, `esc`, `space w h`).
//! * [`check`] — the key conflict checker; the tests run it on the defaults.
//! * [`doc`] — renders `docs/keybindings.md` from this table and the action registry.
//!
//! Key sequences (`g g`, `Space w h`) are collected by [`Keymap::feed`]. User key bindings
//! (`[keymap.<ctx>]`) build on this table later and go through the same checker.

pub mod check;
pub mod doc;
pub mod keys;

pub use check::{Conflict, ConflictKind, check};
pub use keys::{KeyChord, KeyError, parse_keys};

use crate::action::{Action, CommandLineAction};
use slakio_core::i18n::Label;

/// A key context: where the keyboard is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ctx {
    /// Applies everywhere a context of its own does not take the key (text inputs excepted).
    Root,
    /// The non-text regions of the main screen.
    Shell,
    /// The rail: workspaces and views.
    Rail,
    /// The list panel: sections, channels, DMs.
    List,
    /// The work area in Normal mode.
    PaneNormal,
    /// The `:` command line (text input).
    CommandLine,
}

impl Ctx {
    /// Every context, in the order the docs list them.
    pub const ALL: &'static [Ctx] = &[Ctx::Root, Ctx::Shell, Ctx::Rail, Ctx::List, Ctx::PaneNormal, Ctx::CommandLine];

    /// The id config files and the docs use.
    pub fn id(self) -> &'static str {
        match self {
            Ctx::Root => "root",
            Ctx::Shell => "shell",
            Ctx::Rail => "rail",
            Ctx::List => "list",
            Ctx::PaneNormal => "pane.normal",
            Ctx::CommandLine => "cmdline",
        }
    }

    pub fn label(self) -> Label {
        match self {
            Ctx::Root => Label::CtxRoot,
            Ctx::Shell => Label::CtxShell,
            Ctx::Rail => Label::CtxRail,
            Ctx::List => Label::CtxList,
            Ctx::PaneNormal => Label::CtxPaneNormal,
            Ctx::CommandLine => Label::CtxCmdline,
        }
    }

    /// The context a key not bound here is looked up in next. A text input has none: the key
    /// is typed.
    pub fn parent(self) -> Option<Ctx> {
        match self {
            Ctx::Root | Ctx::CommandLine => None,
            Ctx::Shell => Some(Ctx::Root),
            Ctx::Rail | Ctx::List | Ctx::PaneNormal => Some(Ctx::Shell),
        }
    }

    /// This context and its parents, innermost first.
    pub fn chain(self) -> Vec<Ctx> {
        std::iter::successors(Some(self), |c| c.parent()).collect()
    }

    /// Characters typed here are text, not commands.
    pub fn is_text_input(self) -> bool {
        matches!(self, Ctx::CommandLine)
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
}

const fn bind(ctx: Ctx, keys: &'static str, action: Action) -> Binding {
    Binding { ctx, keys, action }
}

/// The default bindings.
pub const DEFAULTS: &[Binding] = &[
    bind(Ctx::Root, ":", Action::CommandLine(CommandLineAction::Open)),
    bind(Ctx::CommandLine, "enter", Action::CommandLine(CommandLineAction::Run)),
    bind(Ctx::CommandLine, "esc", Action::CommandLine(CommandLineAction::Cancel)),
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

impl Default for Keymap {
    fn default() -> Self {
        let bound = DEFAULTS
            .iter()
            .map(|b| Bound {
                ctx: b.ctx,
                keys: parse_keys(b.keys).expect("default key notation is valid (tested)"),
                action: b.action,
            })
            .collect();
        Self { bound }
    }
}

impl Keymap {
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

    /// The key sequences that run `action` from `ctx` (innermost first), for hints.
    pub fn keys_for(&self, action: Action, ctx: Ctx) -> Vec<Vec<KeyChord>> {
        ctx.chain()
            .into_iter()
            .flat_map(|c| self.bound.iter().filter(move |b| b.ctx == c && b.action == action))
            .map(|b| b.keys.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests;
