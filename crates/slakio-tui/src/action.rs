//! The action registry: every action the user can take, once. The key map, the `:` command
//! line, the key help and `docs/keybindings.md` all read it, so an action has one id, one label
//! and one set of command names everywhere.
//!
//! An [`Action`] is namespaced by the part of the state that owns it
//! (`Action::CommandLine(CommandLineAction::Run)`), so dispatching one is routing, never a
//! single match over every action of the app.

use slakio_core::i18n::Label;

/// Something the user asked for, routed to the sub-state that owns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    App(AppAction),
    CommandLine(CommandLineAction),
}

/// Actions of the app as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AppAction {
    Quit,
}

/// Actions of the `:` command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandLineAction {
    Open,
    Run,
    Cancel,
}

/// One registered action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionSpec {
    pub action: Action,
    /// Stable id: config files (`[keymap.<ctx>]`) and the docs name the action by it.
    pub id: &'static str,
    pub label: Label,
    /// Names that run it from the `:` command line (the first is the one the docs show).
    pub commands: &'static [&'static str],
}

/// Every action, in the order the docs list them.
pub const REGISTRY: &[ActionSpec] = &[
    ActionSpec {
        action: Action::App(AppAction::Quit),
        id: "app.quit",
        label: Label::ActionQuit,
        commands: &["qa", "qall", "quitall", "q", "quit"],
    },
    ActionSpec {
        action: Action::CommandLine(CommandLineAction::Open),
        id: "cmdline.open",
        label: Label::ActionCmdlineOpen,
        commands: &[],
    },
    ActionSpec {
        action: Action::CommandLine(CommandLineAction::Run),
        id: "cmdline.run",
        label: Label::ActionCmdlineRun,
        commands: &[],
    },
    ActionSpec {
        action: Action::CommandLine(CommandLineAction::Cancel),
        id: "cmdline.cancel",
        label: Label::ActionCmdlineCancel,
        commands: &[],
    },
];

/// The registry entry of `action` (every action has one; a test checks it).
pub fn spec(action: Action) -> &'static ActionSpec {
    REGISTRY.iter().find(|s| s.action == action).expect("every action is registered")
}

/// The action a `:` command names (surrounding spaces ignored), if any.
pub fn by_command(name: &str) -> Option<Action> {
    let name = name.trim();
    REGISTRY.iter().find(|s| s.commands.contains(&name)).map(|s| s.action)
}

#[cfg(test)]
mod tests;
