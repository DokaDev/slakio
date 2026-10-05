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
    Pane(PaneAction),
    Composer(ComposerAction),
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

/// Actions of the work area's panes (Normal and VISUAL mode) and its history.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaneAction {
    /// The next (newer) message; extends a VISUAL range.
    Next,
    Prev,
    /// The oldest message (loading the rest of the history).
    First,
    /// The newest message.
    Last,
    /// Open the selected message's thread in the thread panel.
    OpenThread,
    /// Start or end a VISUAL range of messages.
    Visual,
    /// Copy the selected messages (the VISUAL range, or the selected one).
    Copy,
    /// Leave VISUAL mode.
    Escape,
    /// Write in the pane's composer (Insert mode).
    Insert,
    /// Close the focused pane: the thread panel, else the conversation.
    Close,
    /// The conversation open before (back/forward history of the work area).
    Back,
    Forward,
}

/// Actions of a composer in Insert mode. Other keys type text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComposerAction {
    Send,
    Newline,
    /// Back to Normal mode; the text stays.
    Leave,
    DeleteWord,
    DeleteLine,
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
    ActionSpec { action: Action::Pane(PaneAction::Next), id: "pane.next", label: Label::ActionPaneNext, commands: &[] },
    ActionSpec { action: Action::Pane(PaneAction::Prev), id: "pane.prev", label: Label::ActionPanePrev, commands: &[] },
    ActionSpec {
        action: Action::Pane(PaneAction::First),
        id: "pane.first",
        label: Label::ActionPaneFirst,
        commands: &[],
    },
    ActionSpec { action: Action::Pane(PaneAction::Last), id: "pane.last", label: Label::ActionPaneLast, commands: &[] },
    ActionSpec {
        action: Action::Pane(PaneAction::OpenThread),
        id: "pane.open_thread",
        label: Label::ActionPaneOpenThread,
        commands: &[],
    },
    ActionSpec {
        action: Action::Pane(PaneAction::Visual),
        id: "pane.visual",
        label: Label::ActionPaneVisual,
        commands: &[],
    },
    ActionSpec { action: Action::Pane(PaneAction::Copy), id: "pane.copy", label: Label::ActionPaneCopy, commands: &[] },
    ActionSpec {
        action: Action::Pane(PaneAction::Escape),
        id: "pane.escape",
        label: Label::ActionPaneEscape,
        commands: &[],
    },
    ActionSpec {
        action: Action::Pane(PaneAction::Insert),
        id: "pane.insert",
        label: Label::ActionPaneInsert,
        commands: &[],
    },
    ActionSpec {
        action: Action::Pane(PaneAction::Close),
        id: "pane.close",
        label: Label::ActionPaneClose,
        commands: &["close"],
    },
    ActionSpec {
        action: Action::Pane(PaneAction::Back),
        id: "history.back",
        label: Label::ActionHistoryBack,
        commands: &["back"],
    },
    ActionSpec {
        action: Action::Pane(PaneAction::Forward),
        id: "history.forward",
        label: Label::ActionHistoryForward,
        commands: &["forward"],
    },
    ActionSpec {
        action: Action::Composer(ComposerAction::Send),
        id: "composer.send",
        label: Label::ActionComposerSend,
        commands: &[],
    },
    ActionSpec {
        action: Action::Composer(ComposerAction::Newline),
        id: "composer.newline",
        label: Label::ActionComposerNewline,
        commands: &[],
    },
    ActionSpec {
        action: Action::Composer(ComposerAction::Leave),
        id: "composer.leave",
        label: Label::ActionComposerLeave,
        commands: &[],
    },
    ActionSpec {
        action: Action::Composer(ComposerAction::DeleteWord),
        id: "composer.delete_word",
        label: Label::ActionComposerDeleteWord,
        commands: &[],
    },
    ActionSpec {
        action: Action::Composer(ComposerAction::DeleteLine),
        id: "composer.delete_line",
        label: Label::ActionComposerDeleteLine,
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
