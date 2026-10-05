//! Context key map: one table of default bindings ([`DEFAULTS`]) decides what a key does in the
//! current [`Ctx`]. A key is looked up in the current context, then in its parents; a text
//! input context has no parent, so every key no binding of its own claims is typed.
//!
//! * [`keys`] — key chords and their notation (`ctrl+e`, `esc`, `:`).
//! * [`doc`] — renders `docs/keybindings.md` from this table and the action registry.
//!
//! User key bindings (`[keymap.<ctx>]`), key sequences, the leader key and the conflict
//! checker build on this table.

pub mod doc;
pub mod keys;

pub use keys::{KeyChord, KeyError, parse_keys};

use crate::action::{Action, CommandLineAction};
use slakio_core::i18n::Label;

/// A key context: where the keyboard is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ctx {
    /// Applies everywhere a context of its own does not take the key.
    Root,
    /// The `:` command line (text input).
    CommandLine,
}

impl Ctx {
    /// Every context, in the order the docs list them.
    pub const ALL: &'static [Ctx] = &[Ctx::Root, Ctx::CommandLine];

    /// The id config files and the docs use.
    pub fn id(self) -> &'static str {
        match self {
            Ctx::Root => "root",
            Ctx::CommandLine => "cmdline",
        }
    }

    pub fn label(self) -> Label {
        match self {
            Ctx::Root => Label::CtxRoot,
            Ctx::CommandLine => Label::CtxCmdline,
        }
    }

    /// The context a key not bound here is looked up in next. A text input has none: the key
    /// is typed.
    pub fn parent(self) -> Option<Ctx> {
        match self {
            Ctx::Root => None,
            Ctx::CommandLine => None,
        }
    }

    /// Characters typed here are text, not commands.
    pub fn is_text_input(self) -> bool {
        matches!(self, Ctx::CommandLine)
    }
}

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

/// The bindings in effect, parsed.
#[derive(Clone, Debug)]
pub struct Keymap {
    bound: Vec<(Ctx, Vec<KeyChord>, Action)>,
}

impl Default for Keymap {
    fn default() -> Self {
        let bound = DEFAULTS
            .iter()
            .map(|b| (b.ctx, parse_keys(b.keys).expect("default key notation is valid (tested)"), b.action))
            .collect();
        Self { bound }
    }
}

impl Keymap {
    /// What `key` does in `ctx`: the action bound in `ctx` or the nearest parent that binds
    /// it, or `None` (the key goes to whatever has the focus).
    pub fn resolve(&self, ctx: Ctx, key: KeyChord) -> Option<Action> {
        let mut at = Some(ctx);
        while let Some(c) = at {
            if let Some((_, _, a)) = self.bound.iter().find(|(bc, keys, _)| *bc == c && keys.as_slice() == [key]) {
                return Some(*a);
            }
            at = c.parent();
        }
        None
    }

    /// The bindings of `ctx`, in table order: (keys, action).
    pub fn bindings(&self, ctx: Ctx) -> impl Iterator<Item = (&[KeyChord], Action)> {
        self.bound.iter().filter(move |(c, _, _)| *c == ctx).map(|(_, k, a)| (k.as_slice(), *a))
    }
}

#[cfg(test)]
mod tests;
