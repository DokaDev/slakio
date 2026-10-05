//! The action registry: every action the user can take, once. The key map, the `:` command
//! line, the key help and `docs/keybindings.md` all read it, so an action has one id, one label
//! and one set of command names everywhere.
//!
//! An [`Action`] is namespaced by the part of the state that owns it
//! (`Action::CommandLine(CommandLineAction::Run)`), so dispatching one is routing, never a
//! single match over every action of the app.

use crate::app::shell::View;
use slakio_core::i18n::Label;

/// Something the user asked for, routed to the sub-state that owns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    App(AppAction),
    CommandLine(CommandLineAction),
    Shell(ShellAction),
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

/// Actions of the shell: focus between the regions, the rail, the list panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShellAction {
    FocusLeft,
    FocusRight,
    RailNext,
    RailPrev,
    /// Show the rail item under the cursor in the list panel.
    RailSelect,
    ListNext,
    ListPrev,
    ListFirst,
    ListLast,
    /// Open the conversation under the cursor, or fold its section.
    ListOpen,
    /// Show or hide the list panel.
    ToggleList,
    /// Show a view of the rail in the list panel.
    Show(View),
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
    ActionSpec {
        action: Action::Shell(ShellAction::FocusLeft),
        id: "focus.left",
        label: Label::ActionFocusLeft,
        commands: &[],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::FocusRight),
        id: "focus.right",
        label: Label::ActionFocusRight,
        commands: &[],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::RailNext),
        id: "rail.next",
        label: Label::ActionRailNext,
        commands: &[],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::RailPrev),
        id: "rail.prev",
        label: Label::ActionRailPrev,
        commands: &[],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::RailSelect),
        id: "rail.select",
        label: Label::ActionRailSelect,
        commands: &[],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::ListNext),
        id: "list.next",
        label: Label::ActionListNext,
        commands: &[],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::ListPrev),
        id: "list.prev",
        label: Label::ActionListPrev,
        commands: &[],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::ListFirst),
        id: "list.first",
        label: Label::ActionListFirst,
        commands: &[],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::ListLast),
        id: "list.last",
        label: Label::ActionListLast,
        commands: &[],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::ListOpen),
        id: "list.open",
        label: Label::ActionListOpen,
        commands: &[],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::ToggleList),
        id: "list.toggle_panel",
        label: Label::ActionListTogglePanel,
        commands: &["list"],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::Show(View::Home)),
        id: "view.home",
        label: Label::ActionViewHome,
        commands: &["home"],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::Show(View::Dms)),
        id: "view.dms",
        label: Label::ActionViewDms,
        commands: &["dms"],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::Show(View::Activity)),
        id: "view.activity",
        label: Label::ActionViewActivity,
        commands: &["activity"],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::Show(View::Files)),
        id: "view.files",
        label: Label::ActionViewFiles,
        commands: &["files"],
    },
    ActionSpec {
        action: Action::Shell(ShellAction::Show(View::Later)),
        id: "view.later",
        label: Label::ActionViewLater,
        commands: &["later"],
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
