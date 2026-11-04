//! The key contexts: where the keyboard is, and which context a key not bound there is looked
//! up in next.

use slakio_core::i18n::Label;

/// A key context: where the keyboard is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ctx {
    /// Looked up before any other context, also in text inputs and modal ones.
    Global,
    /// Applies everywhere a context of its own does not take the key (text inputs and modal
    /// contexts excepted).
    Root,
    /// The sign-in screen's picker of the Slack desktop app's workspaces.
    SignIn,
    /// The non-text regions of the main screen.
    Shell,
    /// The view switcher: the list panel's first rows, a row per view.
    ViewSwitcher,
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
    /// Picking a message's link: letters on the links, or their list (modal).
    Links,
}

impl Ctx {
    /// Every context, in the order the docs list them.
    pub const ALL: &'static [Ctx] = &[
        Ctx::Global,
        Ctx::Root,
        Ctx::SignIn,
        Ctx::Shell,
        Ctx::ViewSwitcher,
        Ctx::List,
        Ctx::PaneNormal,
        Ctx::PaneVisual,
        Ctx::CommandLine,
        Ctx::ComposerInsert,
        Ctx::Help,
        Ctx::HelpFilter,
        Ctx::Dialog,
        Ctx::Switcher,
        Ctx::Links,
    ];

    /// The id config files and the docs use.
    pub fn id(self) -> &'static str {
        match self {
            Ctx::Global => "global",
            Ctx::Root => "root",
            Ctx::SignIn => "signin",
            Ctx::Shell => "shell",
            Ctx::ViewSwitcher => "nav",
            Ctx::List => "list",
            Ctx::PaneNormal => "pane.normal",
            Ctx::PaneVisual => "pane.visual",
            Ctx::CommandLine => "cmdline",
            Ctx::ComposerInsert => "composer.insert",
            Ctx::Help => "help",
            Ctx::HelpFilter => "help.filter",
            Ctx::Dialog => "dialog",
            Ctx::Switcher => "switcher",
            Ctx::Links => "links",
        }
    }

    pub fn label(self) -> Label {
        match self {
            Ctx::Global => Label::CtxGlobal,
            Ctx::Root => Label::CtxRoot,
            Ctx::SignIn => Label::CtxSignin,
            Ctx::Shell => Label::CtxShell,
            Ctx::ViewSwitcher => Label::CtxNav,
            Ctx::List => Label::CtxList,
            Ctx::PaneNormal => Label::CtxPaneNormal,
            Ctx::PaneVisual => Label::CtxPaneVisual,
            Ctx::CommandLine => Label::CtxCmdline,
            Ctx::ComposerInsert => Label::CtxComposerInsert,
            Ctx::Help => Label::CtxHelp,
            Ctx::HelpFilter => Label::CtxHelpFilter,
            Ctx::Dialog => Label::CtxDialog,
            Ctx::Switcher => Label::CtxSwitcher,
            Ctx::Links => Label::CtxLinks,
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
            | Ctx::Switcher
            | Ctx::Links => None,
            Ctx::Shell | Ctx::SignIn => Some(Ctx::Root),
            Ctx::ViewSwitcher | Ctx::List | Ctx::PaneNormal => Some(Ctx::Shell),
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
        matches!(self, Ctx::Help | Ctx::Dialog | Ctx::Switcher | Ctx::Links)
    }
}
