//! The action registry: every action the user can take, once. The key map, the `:` command
//! line, the hint line, the key help and `docs/keybindings.md` all read it, so an action has one
//! id, one label and one set of command names everywhere.
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
    Help(HelpAction),
    Dialog(DialogAction),
}

/// Actions of the app as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AppAction {
    /// Quit; asks first when a composer holds text not sent.
    Quit,
    /// `Ctrl+C`: cancel what is pending and say how to quit.
    Interrupt,
    /// The command palette (the `:` command line); closes it when it is open.
    Palette,
    /// Pick another workspace (on the rail).
    ChooseWorkspace,
}

/// Actions of the `:` command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandLineAction {
    Open,
    Run,
    Cancel,
    /// The next entry of the list under the input (round again past the last).
    Next,
    Prev,
}

/// Actions of the shell: focus between the panels, the rail, the list panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShellAction {
    FocusLeft,
    FocusRight,
    /// No panel is above or below another yet (splits come later): says so.
    FocusUp,
    FocusDown,
    /// The next panel: rail, list, main pane, thread panel, round again.
    FocusNext,
    FocusPrev,
    /// Go to the rail (it expands while it has the focus); from the rail, back to the list.
    FocusRail,
    RailNext,
    RailPrev,
    RailFirst,
    RailLast,
    /// Show the rail item under the cursor in the list panel.
    RailSelect,
    /// Back to the list panel, showing nothing new.
    RailLeave,
    ListNext,
    ListPrev,
    ListFirst,
    ListLast,
    ListHalfDown,
    ListHalfUp,
    ListPageDown,
    ListPageUp,
    /// Open the conversation under the cursor and move there, or fold its section.
    ListOpen,
    /// Open the conversation under the cursor and stay in the list, or unfold its section.
    ListPeek,
    /// To the conversation's section header; fold the section; from a folded one, the rail.
    ListLeft,
    ListSectionPrev,
    ListSectionNext,
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
    HalfDown,
    HalfUp,
    PageDown,
    PageUp,
    /// `Enter`: the selected message's thread in the thread panel; with none selected, write.
    OpenThread,
    /// Start or end a VISUAL range of messages.
    Visual,
    /// Copy the selected messages (the VISUAL range, or the selected one).
    Copy,
    /// One step out: VISUAL, the selection, the thread panel, the main pane.
    Escape,
    /// Write in the pane's composer (Insert mode).
    Insert,
    /// Close the focused pane: the thread panel, else the conversation.
    Close,
    /// The panel to the left (the main pane from the thread panel, else the list).
    Left,
    /// The thread panel from the main pane.
    Right,
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
    DeleteToEnd,
}

/// Actions of the keyboard help.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HelpAction {
    /// Open the help for where the keyboard is (closes it when open).
    Open,
    Close,
    Next,
    Prev,
    PageDown,
    PageUp,
    First,
    Last,
    /// Run the action of the row, or open or close a section.
    Run,
    Expand,
    Collapse,
    /// Type to filter the rows.
    Search,
    SearchDone,
    SearchCancel,
}

/// Actions of a question with two answers (quit with text not sent? use icons?).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DialogAction {
    Yes,
    No,
    /// The answer that has the focus (`No` first).
    Choose,
    /// Move the focus to the other answer.
    Toggle,
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

const fn spec_of(action: Action, id: &'static str, label: Label, commands: &'static [&'static str]) -> ActionSpec {
    ActionSpec { action, id, label, commands }
}

const fn app(a: AppAction, id: &'static str, label: Label, commands: &'static [&'static str]) -> ActionSpec {
    spec_of(Action::App(a), id, label, commands)
}

const fn shell(a: ShellAction, id: &'static str, label: Label, commands: &'static [&'static str]) -> ActionSpec {
    spec_of(Action::Shell(a), id, label, commands)
}

const fn pane(a: PaneAction, id: &'static str, label: Label, commands: &'static [&'static str]) -> ActionSpec {
    spec_of(Action::Pane(a), id, label, commands)
}

const fn composer(a: ComposerAction, id: &'static str, label: Label) -> ActionSpec {
    spec_of(Action::Composer(a), id, label, &[])
}

const fn help(a: HelpAction, id: &'static str, label: Label, commands: &'static [&'static str]) -> ActionSpec {
    spec_of(Action::Help(a), id, label, commands)
}

const fn dialog(a: DialogAction, id: &'static str, label: Label) -> ActionSpec {
    spec_of(Action::Dialog(a), id, label, &[])
}

/// Every action, in the order the docs and the key help list them.
pub const REGISTRY: &[ActionSpec] = &[
    app(AppAction::Quit, "app.quit", Label::ActionQuit, &["qa", "qall", "quitall", "q", "quit"]),
    app(AppAction::Interrupt, "app.interrupt", Label::ActionInterrupt, &[]),
    app(AppAction::Palette, "palette.open", Label::ActionPalette, &[]),
    app(AppAction::ChooseWorkspace, "workspace.choose", Label::ActionWorkspaceChoose, &["workspace"]),
    help(HelpAction::Open, "help.open", Label::ActionHelpOpen, &["help"]),
    spec_of(Action::CommandLine(CommandLineAction::Open), "cmdline.open", Label::ActionCmdlineOpen, &[]),
    spec_of(Action::CommandLine(CommandLineAction::Run), "cmdline.run", Label::ActionCmdlineRun, &[]),
    spec_of(Action::CommandLine(CommandLineAction::Cancel), "cmdline.cancel", Label::ActionCmdlineCancel, &[]),
    spec_of(Action::CommandLine(CommandLineAction::Next), "cmdline.next", Label::ActionCmdlineNext, &[]),
    spec_of(Action::CommandLine(CommandLineAction::Prev), "cmdline.prev", Label::ActionCmdlinePrev, &[]),
    shell(ShellAction::FocusNext, "focus.next", Label::ActionFocusNext, &[]),
    shell(ShellAction::FocusPrev, "focus.prev", Label::ActionFocusPrev, &[]),
    shell(ShellAction::FocusLeft, "focus.left", Label::ActionFocusLeft, &[]),
    shell(ShellAction::FocusDown, "focus.down", Label::ActionFocusDown, &[]),
    shell(ShellAction::FocusUp, "focus.up", Label::ActionFocusUp, &[]),
    shell(ShellAction::FocusRight, "focus.right", Label::ActionFocusRight, &[]),
    shell(ShellAction::FocusRail, "rail.focus", Label::ActionRailFocus, &["rail"]),
    shell(ShellAction::RailNext, "rail.next", Label::ActionRailNext, &[]),
    shell(ShellAction::RailPrev, "rail.prev", Label::ActionRailPrev, &[]),
    shell(ShellAction::RailFirst, "rail.first", Label::ActionRailFirst, &[]),
    shell(ShellAction::RailLast, "rail.last", Label::ActionRailLast, &[]),
    shell(ShellAction::RailSelect, "rail.select", Label::ActionRailSelect, &[]),
    shell(ShellAction::RailLeave, "rail.leave", Label::ActionRailLeave, &[]),
    shell(ShellAction::ListNext, "list.next", Label::ActionListNext, &[]),
    shell(ShellAction::ListPrev, "list.prev", Label::ActionListPrev, &[]),
    shell(ShellAction::ListFirst, "list.first", Label::ActionListFirst, &[]),
    shell(ShellAction::ListLast, "list.last", Label::ActionListLast, &[]),
    shell(ShellAction::ListHalfDown, "list.half_down", Label::ActionListHalfDown, &[]),
    shell(ShellAction::ListHalfUp, "list.half_up", Label::ActionListHalfUp, &[]),
    shell(ShellAction::ListPageDown, "list.page_down", Label::ActionListPageDown, &[]),
    shell(ShellAction::ListPageUp, "list.page_up", Label::ActionListPageUp, &[]),
    shell(ShellAction::ListOpen, "list.open", Label::ActionListOpen, &[]),
    shell(ShellAction::ListPeek, "list.peek", Label::ActionListPeek, &[]),
    shell(ShellAction::ListLeft, "list.fold", Label::ActionListFold, &[]),
    shell(ShellAction::ListSectionPrev, "list.section_prev", Label::ActionListSectionPrev, &[]),
    shell(ShellAction::ListSectionNext, "list.section_next", Label::ActionListSectionNext, &[]),
    shell(ShellAction::ToggleList, "list.toggle_panel", Label::ActionListTogglePanel, &["list"]),
    shell(ShellAction::Show(View::Home), "view.home", Label::ActionViewHome, &["home"]),
    shell(ShellAction::Show(View::Dms), "view.dms", Label::ActionViewDms, &["dms"]),
    shell(ShellAction::Show(View::Activity), "view.activity", Label::ActionViewActivity, &["activity"]),
    shell(ShellAction::Show(View::Files), "view.files", Label::ActionViewFiles, &["files"]),
    shell(ShellAction::Show(View::Later), "view.later", Label::ActionViewLater, &["later"]),
    pane(PaneAction::Next, "pane.next", Label::ActionPaneNext, &[]),
    pane(PaneAction::Prev, "pane.prev", Label::ActionPanePrev, &[]),
    pane(PaneAction::First, "pane.first", Label::ActionPaneFirst, &[]),
    pane(PaneAction::Last, "pane.last", Label::ActionPaneLast, &[]),
    pane(PaneAction::HalfDown, "pane.half_down", Label::ActionPaneHalfDown, &[]),
    pane(PaneAction::HalfUp, "pane.half_up", Label::ActionPaneHalfUp, &[]),
    pane(PaneAction::PageDown, "pane.page_down", Label::ActionPanePageDown, &[]),
    pane(PaneAction::PageUp, "pane.page_up", Label::ActionPanePageUp, &[]),
    pane(PaneAction::OpenThread, "pane.open_thread", Label::ActionPaneOpenThread, &[]),
    pane(PaneAction::Insert, "pane.insert", Label::ActionPaneInsert, &[]),
    pane(PaneAction::Visual, "pane.visual", Label::ActionPaneVisual, &[]),
    pane(PaneAction::Copy, "pane.copy", Label::ActionPaneCopy, &[]),
    pane(PaneAction::Escape, "pane.escape", Label::ActionPaneEscape, &[]),
    pane(PaneAction::Left, "pane.left", Label::ActionPaneLeft, &[]),
    pane(PaneAction::Right, "pane.right", Label::ActionPaneRight, &[]),
    pane(PaneAction::Close, "pane.close", Label::ActionPaneClose, &["close"]),
    pane(PaneAction::Back, "history.back", Label::ActionHistoryBack, &["back"]),
    pane(PaneAction::Forward, "history.forward", Label::ActionHistoryForward, &["forward"]),
    composer(ComposerAction::Send, "composer.send", Label::ActionComposerSend),
    composer(ComposerAction::Newline, "composer.newline", Label::ActionComposerNewline),
    composer(ComposerAction::Leave, "composer.leave", Label::ActionComposerLeave),
    composer(ComposerAction::DeleteWord, "composer.delete_word", Label::ActionComposerDeleteWord),
    composer(ComposerAction::DeleteLine, "composer.delete_line", Label::ActionComposerDeleteLine),
    composer(ComposerAction::DeleteToEnd, "composer.delete_to_end", Label::ActionComposerDeleteToEnd),
    help(HelpAction::Close, "help.close", Label::ActionHelpClose, &[]),
    help(HelpAction::Next, "help.next", Label::ActionHelpNext, &[]),
    help(HelpAction::Prev, "help.prev", Label::ActionHelpPrev, &[]),
    help(HelpAction::PageDown, "help.page_down", Label::ActionHelpPageDown, &[]),
    help(HelpAction::PageUp, "help.page_up", Label::ActionHelpPageUp, &[]),
    help(HelpAction::First, "help.first", Label::ActionHelpFirst, &[]),
    help(HelpAction::Last, "help.last", Label::ActionHelpLast, &[]),
    help(HelpAction::Run, "help.run", Label::ActionHelpRun, &[]),
    help(HelpAction::Expand, "help.expand", Label::ActionHelpExpand, &[]),
    help(HelpAction::Collapse, "help.collapse", Label::ActionHelpCollapse, &[]),
    help(HelpAction::Search, "help.search", Label::ActionHelpSearch, &[]),
    help(HelpAction::SearchDone, "help.search_done", Label::ActionHelpSearchDone, &[]),
    help(HelpAction::SearchCancel, "help.search_cancel", Label::ActionHelpSearchCancel, &[]),
    dialog(DialogAction::Yes, "dialog.yes", Label::ActionDialogYes),
    dialog(DialogAction::No, "dialog.no", Label::ActionDialogNo),
    dialog(DialogAction::Choose, "dialog.choose", Label::ActionDialogChoose),
    dialog(DialogAction::Toggle, "dialog.toggle", Label::ActionDialogToggle),
];

/// The registry entry of `action` (every action has one; a test checks it).
pub fn spec(action: Action) -> &'static ActionSpec {
    REGISTRY.iter().find(|s| s.action == action).expect("every action is registered")
}

/// Fuzzy subsequence score of `query` in `text` (case and spaces in `query` ignored); `None`
/// when `query` is not a subsequence of `text`. The first character of `query` must start a
/// word of `text`, so a short word does not pick an action that merely holds its letters.
/// Higher is better: bonuses for word starts and runs, a cost for gaps and a late start.
pub fn word_score(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let boundary = |i: usize| i == 0 || matches!(t[i - 1], ' ' | '.' | '_' | '-' | '/' | '(' | ':');
    let mut best: Option<i32> = None;
    // Every start of the first query character at a word start; greedy after it.
    for start in (0..t.len()).filter(|&i| t[i] == q[0] && boundary(i)) {
        let (mut score, mut qi, mut prev) = (0i32, 0, None::<usize>);
        for (i, &c) in t.iter().enumerate().skip(start) {
            if qi < q.len() && c == q[qi] {
                score += 1 + if boundary(i) { 8 } else { 0 };
                score += match prev {
                    Some(p) if p + 1 == i => 5,
                    Some(p) => -((i - p - 1) as i32).min(3),
                    None => -(start as i32).min(5),
                };
                prev = Some(i);
                qi += 1;
            }
        }
        if qi == q.len() {
            best = Some(best.map_or(score, |b| b.max(score)));
        }
    }
    best
}

/// Registry indices whose label (in the UI language or in English) or id matches `query`
/// ([`word_score`]), best first; ties keep the registry's order. English words find actions
/// while the UI is in Korean too.
pub fn search(query: &str, i18n: &slakio_core::i18n::I18n) -> Vec<usize> {
    let english = slakio_core::i18n::I18n::new(slakio_core::i18n::Lang::En);
    let mut hits: Vec<(i32, usize)> = REGISTRY
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let (own, en) = (i18n.label(s.label).to_string(), english.label(s.label).to_string());
            [own.as_str(), en.as_str(), s.id].iter().filter_map(|t| word_score(query, t)).max().map(|sc| (sc, i))
        })
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.into_iter().map(|(_, i)| i).collect()
}

/// The action a `:` command names (surrounding spaces ignored), if any.
pub fn by_command(name: &str) -> Option<Action> {
    let name = name.trim();
    REGISTRY.iter().find(|s| s.commands.contains(&name)).map(|s| s.action)
}

#[cfg(test)]
mod tests;
