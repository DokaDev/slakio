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
    Tab(TabAction),
    Composer(ComposerAction),
    Help(HelpAction),
    Dialog(DialogAction),
    SignIn(SignInAction),
    Links(LinksAction),
    Search(SearchAction),
}

impl Action {
    /// The action asks the backend to change something (a to-do, or writing a message to
    /// send): a backend that changes nothing offers none of them.
    pub fn changes(self) -> bool {
        matches!(
            self,
            Action::Shell(ShellAction::TodoToggle | ShellAction::TodoMove | ShellAction::TodoClear)
                | Action::Pane(PaneAction::Insert)
                | Action::Composer(ComposerAction::Send)
        )
    }
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
    /// Pick another workspace (the switcher under the list panel's workspace chip).
    ChooseWorkspace,
    /// Ask again for what could not be loaded, when the reason may pass (`:retry`).
    Retry,
    /// Picture people by their initials chip, or not (`:avatars`); saved in the config file.
    ToggleAvatars,
    /// Nerd Font icons, or text instead (`:icons`); saved in the config file.
    ToggleIcons,
    /// Messages comfortable or compact (`:density`); saved in the config file.
    ToggleDensity,
    /// The network switched off, or on again (`:net`); for this run only.
    ToggleNetwork,
    /// Sign in again while the app runs (`:login`): a workspace Slack signed out, or another.
    SignInAgain,
    /// Search the messages of the workspace the keyboard is in (`:search <words>`).
    Search,
    /// Read the focused conversation from a day (`:jump <date>`); alone, the command line ready
    /// for the day.
    JumpDate,
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

/// Actions of the shell: focus between the panels, the view switcher, the list panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShellAction {
    FocusLeft,
    FocusRight,
    /// The pane above (below) the one with the keyboard; with none, says so.
    FocusUp,
    FocusDown,
    /// The next panel: the views, the list, main pane, thread panel, round again.
    FocusNext,
    FocusPrev,
    /// Go to the views' rows, on the view shown (showing the list panel); from them, back to
    /// the list.
    FocusNav,
    /// The next view's row; from the last, the list's first row.
    NavNext,
    NavPrev,
    NavFirst,
    NavLast,
    /// Show the view under the views' cursor in the list panel; on the folded row, unfold it.
    NavSelect,
    /// Back to the list panel, showing nothing new.
    NavLeave,
    /// The workspace switcher's next and previous workspace, switching to it, closing it.
    SwitcherNext,
    SwitcherPrev,
    SwitcherChoose,
    SwitcherClose,
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
    /// To the conversation's section header; fold the section; from a folded one, the views.
    ListLeft,
    ListSectionPrev,
    ListSectionNext,
    /// Show the next (previous) view in the list panel, round again.
    ViewNext,
    ViewPrev,
    /// Show or hide the list panel.
    ToggleList,
    /// Fold the views to one row, the view shown, or unfold them (`:navrows`); saved in the
    /// config file (`nav_rows`).
    ToggleNavRows,
    /// Show a view in the list panel.
    Show(View),
    /// The to-do under the cursor completed, or a completed one in progress again.
    TodoToggle,
    /// The saved item under the cursor out of To-dos into Saved, or back (in progress).
    TodoMove,
    /// The saved item under the cursor cleared (asks first).
    TodoClear,
    /// Filter To-dos by state or reminder (`:filter <value>`; alone, the values to pick).
    TodoFilter,
    /// Order To-dos (`:sort <value>`; alone, the orders to pick).
    TodoSort,
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
    /// Open the selected message's (first) link in the browser.
    OpenLink,
    /// One step out: VISUAL, the selection, the thread panel, the main pane.
    Escape,
    /// Write in the pane's composer (Insert mode).
    Insert,
    /// Close the focused pane: the thread panel, else the conversation.
    Close,
    /// The pane to the left, by place (from the leftmost, the list).
    Left,
    /// The pane to the right, by place.
    Right,
    /// Split the pane: the selected message's thread (else what it shows) beside it, or under
    /// it; the thread panel splits off as a pane of its own. From the list (`Space w v`), what
    /// is under its cursor.
    SplitRight,
    SplitBelow,
    /// From the list: what is under its cursor, in a new pane beside the one it opens into.
    OpenBeside,
    /// The active pane takes the whole tab, or every pane shows again.
    Zoom,
    /// The panes of the tab shown share its room evenly.
    Equalize,
    /// The active pane wider, narrower, taller or shorter (its neighbour gives or takes).
    Wider,
    Narrower,
    Taller,
    Shorter,
    /// What the pane showed before (each pane has its own back/forward history).
    Back,
    Forward,
}

/// Actions on the tabs of the work area. The app never reorders tabs; only these moves do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TabAction {
    /// `t`: what is under the list's cursor, or the selected message's thread, in a new tab
    /// (focused instead when it is open already).
    Open,
    Next,
    Prev,
    /// Show tab `n` (1 to 9).
    Go(u8),
    /// Close the tab shown and its panes.
    Close,
    /// Open the tab closed last again, where it was.
    Reopen,
    /// Name the tab shown (`:rename <name>`; an empty name: named after what it shows).
    Rename,
    MoveLeft,
    MoveRight,
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

/// Actions while a message's link is picked (`o` on one with several links that open). The
/// letters drawn on the links are not actions: each one is the link it is drawn on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinksAction {
    /// Show the links as a list.
    List,
    /// The list's next link (with letters on the links: show the list).
    Down,
    /// The list's previous link (with letters on the links: show the list).
    Up,
    /// Open the link under the list's cursor.
    Open,
    /// Take the last letter typed back.
    Back,
    /// Stop picking.
    Cancel,
}

/// Actions of the search popup. Other keys type the words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SearchAction {
    /// Search the words typed now; when they are what is shown, open the result under the cursor.
    Run,
    Next,
    Prev,
    /// The next (previous) page of results.
    PageNext,
    PagePrev,
    /// Search the next (previous) workspace signed in to.
    ScopeNext,
    ScopePrev,
    /// Clear the words.
    Clear,
    Close,
}

/// Actions of the sign-in screen's picker of the Slack desktop app's workspaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SignInAction {
    Down,
    Up,
    /// Sign in to the marked workspaces, else to the one under the cursor.
    Pick,
    /// Mark the workspace under the cursor (or unmark it) to sign in to several at once.
    Mark,
    /// Paste a browser request instead.
    Paste,
    /// Back to the app (the sign-in screen over it, `:login`), signed in as before.
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

const fn tab(a: TabAction, id: &'static str, label: Label, commands: &'static [&'static str]) -> ActionSpec {
    spec_of(Action::Tab(a), id, label, commands)
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

const fn find(a: SearchAction, id: &'static str, label: Label) -> ActionSpec {
    spec_of(Action::Search(a), id, label, &[])
}

/// Every action, in the order the docs and the key help list them.
pub const REGISTRY: &[ActionSpec] = &[
    app(AppAction::Quit, "app.quit", Label::ActionQuit, &["qa", "qall", "quitall"]),
    app(AppAction::Interrupt, "app.interrupt", Label::ActionInterrupt, &[]),
    app(AppAction::Palette, "palette.open", Label::ActionPalette, &[]),
    app(AppAction::ChooseWorkspace, "workspace.choose", Label::ActionWorkspaceChoose, &["workspace"]),
    app(AppAction::Retry, "app.retry", Label::ActionRetry, &["retry"]),
    help(HelpAction::Open, "help.open", Label::ActionHelpOpen, &["help"]),
    app(AppAction::ToggleAvatars, "avatars.toggle", Label::ActionAvatars, &["avatars"]),
    app(AppAction::ToggleIcons, "icons.toggle", Label::ActionIcons, &["icons"]),
    app(AppAction::ToggleDensity, "density.toggle", Label::ActionDensity, &["density"]),
    app(AppAction::ToggleNetwork, "net.toggle", Label::ActionNet, &["net"]),
    app(AppAction::SignInAgain, "signin.again", Label::ActionSigninAgain, &["login"]),
    app(AppAction::Search, "search.open", Label::ActionSearchOpen, &["search"]),
    app(AppAction::JumpDate, "jump.date", Label::ActionJumpDate, &["jump"]),
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
    // `:rail` is the name it had before the top bar, which the view switcher replaced.
    shell(ShellAction::FocusNav, "nav.focus", Label::ActionNavFocus, &["nav", "rail"]),
    shell(ShellAction::NavNext, "nav.next", Label::ActionNavNext, &[]),
    shell(ShellAction::NavPrev, "nav.prev", Label::ActionNavPrev, &[]),
    shell(ShellAction::NavFirst, "nav.first", Label::ActionNavFirst, &[]),
    shell(ShellAction::NavLast, "nav.last", Label::ActionNavLast, &[]),
    shell(ShellAction::NavSelect, "nav.select", Label::ActionNavSelect, &[]),
    shell(ShellAction::NavLeave, "nav.leave", Label::ActionNavLeave, &[]),
    shell(ShellAction::SwitcherNext, "switcher.next", Label::ActionSwitcherNext, &[]),
    shell(ShellAction::SwitcherPrev, "switcher.prev", Label::ActionSwitcherPrev, &[]),
    shell(ShellAction::SwitcherChoose, "switcher.choose", Label::ActionSwitcherChoose, &[]),
    shell(ShellAction::SwitcherClose, "switcher.close", Label::ActionSwitcherClose, &[]),
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
    shell(ShellAction::ViewNext, "view.next", Label::ActionViewNext, &[]),
    shell(ShellAction::ViewPrev, "view.prev", Label::ActionViewPrev, &[]),
    shell(ShellAction::ToggleList, "list.toggle_panel", Label::ActionListTogglePanel, &["list"]),
    shell(ShellAction::ToggleNavRows, "nav.toggle_rows", Label::ActionNavRows, &["navrows"]),
    shell(ShellAction::Show(View::Home), "view.home", Label::ActionViewHome, &["home"]),
    shell(ShellAction::Show(View::Dms), "view.dms", Label::ActionViewDms, &["dms"]),
    shell(ShellAction::Show(View::Activity), "view.activity", Label::ActionViewActivity, &["activity"]),
    shell(ShellAction::Show(View::Files), "view.files", Label::ActionViewFiles, &["files"]),
    // `:later` is To-dos' name before Slack renamed it.
    shell(ShellAction::Show(View::Todos), "view.todos", Label::ActionViewTodos, &["todos", "later"]),
    shell(ShellAction::TodoToggle, "todo.toggle", Label::ActionTodoToggle, &[]),
    shell(ShellAction::TodoMove, "todo.move", Label::ActionTodoMove, &[]),
    shell(ShellAction::TodoClear, "todo.clear", Label::ActionTodoClear, &[]),
    shell(ShellAction::TodoFilter, "todo.filter", Label::ActionTodoFilter, &["filter"]),
    shell(ShellAction::TodoSort, "todo.sort", Label::ActionTodoSort, &["sort"]),
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
    pane(PaneAction::OpenLink, "pane.open_link", Label::ActionPaneOpenLink, &["open"]),
    pane(PaneAction::Escape, "pane.escape", Label::ActionPaneEscape, &[]),
    pane(PaneAction::Left, "pane.left", Label::ActionPaneLeft, &[]),
    pane(PaneAction::Right, "pane.right", Label::ActionPaneRight, &[]),
    pane(PaneAction::SplitRight, "pane.split_right", Label::ActionPaneSplitRight, &["vsplit", "vs"]),
    pane(PaneAction::SplitBelow, "pane.split_below", Label::ActionPaneSplitBelow, &["split", "sp"]),
    pane(PaneAction::OpenBeside, "pane.open_beside", Label::ActionPaneOpenBeside, &[]),
    pane(PaneAction::Zoom, "pane.zoom", Label::ActionPaneZoom, &["zoom"]),
    pane(PaneAction::Equalize, "pane.equalize", Label::ActionPaneEqualize, &["equalize", "equalise"]),
    pane(PaneAction::Wider, "pane.wider", Label::ActionPaneWider, &[]),
    pane(PaneAction::Narrower, "pane.narrower", Label::ActionPaneNarrower, &[]),
    pane(PaneAction::Taller, "pane.taller", Label::ActionPaneTaller, &[]),
    pane(PaneAction::Shorter, "pane.shorter", Label::ActionPaneShorter, &[]),
    // `:q` closes as in vim (the pane, then its tab), never the app: that is `:qa`.
    pane(PaneAction::Close, "pane.close", Label::ActionPaneClose, &["q", "quit", "close"]),
    pane(PaneAction::Back, "history.back", Label::ActionHistoryBack, &["back"]),
    pane(PaneAction::Forward, "history.forward", Label::ActionHistoryForward, &["forward"]),
    tab(TabAction::Open, "tab.open", Label::ActionTabOpen, &[]),
    tab(TabAction::Next, "tab.next", Label::ActionTabNext, &["tabnext", "tabn"]),
    tab(TabAction::Prev, "tab.prev", Label::ActionTabPrev, &["tabprevious", "tabp"]),
    tab(TabAction::Go(1), "tab.go.1", Label::ActionTabGo1, &[]),
    tab(TabAction::Go(2), "tab.go.2", Label::ActionTabGo2, &[]),
    tab(TabAction::Go(3), "tab.go.3", Label::ActionTabGo3, &[]),
    tab(TabAction::Go(4), "tab.go.4", Label::ActionTabGo4, &[]),
    tab(TabAction::Go(5), "tab.go.5", Label::ActionTabGo5, &[]),
    tab(TabAction::Go(6), "tab.go.6", Label::ActionTabGo6, &[]),
    tab(TabAction::Go(7), "tab.go.7", Label::ActionTabGo7, &[]),
    tab(TabAction::Go(8), "tab.go.8", Label::ActionTabGo8, &[]),
    tab(TabAction::Go(9), "tab.go.9", Label::ActionTabGo9, &[]),
    tab(TabAction::Close, "tab.close", Label::ActionTabClose, &["tabclose", "tabc"]),
    tab(TabAction::Reopen, "tab.reopen", Label::ActionTabReopen, &["reopen"]),
    tab(TabAction::Rename, "tab.rename", Label::ActionTabRename, &["rename"]),
    tab(TabAction::MoveLeft, "tab.move_left", Label::ActionTabMoveLeft, &[]),
    tab(TabAction::MoveRight, "tab.move_right", Label::ActionTabMoveRight, &[]),
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
    spec_of(Action::SignIn(SignInAction::Down), "signin.down", Label::ActionSigninDown, &[]),
    spec_of(Action::SignIn(SignInAction::Up), "signin.up", Label::ActionSigninUp, &[]),
    spec_of(Action::SignIn(SignInAction::Pick), "signin.pick", Label::ActionSigninPick, &[]),
    spec_of(Action::SignIn(SignInAction::Mark), "signin.mark", Label::ActionSigninMark, &[]),
    spec_of(Action::SignIn(SignInAction::Paste), "signin.paste", Label::ActionSigninPaste, &[]),
    spec_of(Action::SignIn(SignInAction::Cancel), "signin.cancel", Label::ActionSigninCancel, &[]),
    spec_of(Action::Links(LinksAction::List), "links.list", Label::ActionLinksList, &[]),
    spec_of(Action::Links(LinksAction::Down), "links.down", Label::ActionLinksDown, &[]),
    spec_of(Action::Links(LinksAction::Up), "links.up", Label::ActionLinksUp, &[]),
    spec_of(Action::Links(LinksAction::Open), "links.open", Label::ActionLinksOpen, &[]),
    spec_of(Action::Links(LinksAction::Back), "links.back", Label::ActionLinksBack, &[]),
    spec_of(Action::Links(LinksAction::Cancel), "links.cancel", Label::ActionLinksCancel, &[]),
    find(SearchAction::Run, "search.run", Label::ActionSearchRun),
    find(SearchAction::Next, "search.next", Label::ActionSearchNext),
    find(SearchAction::Prev, "search.prev", Label::ActionSearchPrev),
    find(SearchAction::PageNext, "search.page_next", Label::ActionSearchPageNext),
    find(SearchAction::PagePrev, "search.page_prev", Label::ActionSearchPagePrev),
    find(SearchAction::ScopeNext, "search.scope_next", Label::ActionSearchScopeNext),
    find(SearchAction::ScopePrev, "search.scope_prev", Label::ActionSearchScopePrev),
    find(SearchAction::Clear, "search.clear", Label::ActionSearchClear),
    find(SearchAction::Close, "search.close", Label::ActionSearchClose),
];

/// The registry entry of `action` (every action has one; a test checks it).
pub fn spec(action: Action) -> &'static ActionSpec {
    REGISTRY.iter().find(|s| s.action == action).expect("every action is registered")
}

mod rank;
pub use rank::{by_command, rank, rank_spec, search, word_score};

#[cfg(test)]
mod tests;
