//! The hint line: for each place the keyboard can be, the few keys worth knowing there, best
//! first. Only actions are listed here; the keys come from the key map ([`resolve`]), so a key
//! rebound shows as bound, and an action nothing is bound to is left out.

use super::{Ctx, Keymap, LEADER, check, keys, parse_keys};
use crate::action::{
    Action, AppAction, CommandLineAction, ComposerAction, HelpAction, PaneAction, ShellAction, TabAction,
};
use slakio_core::i18n::Label;

/// Where the keyboard is, as far as the hints care.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// No backend: the welcome screen.
    Welcome,
    /// The list panel on a conversation.
    ListConversation,
    /// The list panel on a section header.
    ListSection,
    /// The list panel with nothing to list (a view of a later version).
    ListEmpty,
    /// The view switcher.
    ViewSwitcher,
    /// The work area with nothing open.
    WorkEmpty,
    /// A main pane, no message selected.
    Pane,
    /// A pane with a message selected.
    PaneSelected,
    /// The thread panel, no message selected.
    Thread,
    /// The thread panel with a reply selected (it has no thread of its own).
    ThreadSelected,
    /// A thread in a pane of its own (a tab), no message selected.
    ThreadPane,
    Visual,
    Insert,
    CommandLine,
}

/// One entry of the hint line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    One(Action, Label),
    /// Two actions under one label (`j/k move`).
    Pair(Action, Action, Label),
    /// The leader key, when the context binds sequences after it.
    Leader(Label),
}

use Hint::{Leader, One, Pair};

const OPEN: Action = Action::Shell(ShellAction::ListOpen);
const NEXT_PANE: Action = Action::Shell(ShellAction::FocusNext);
const NAV: Action = Action::Shell(ShellAction::FocusNav);
/// In the list panel the views are right above: `[` / `]` show the one before or after.
const VIEWS: Hint = Pair(Action::Shell(ShellAction::ViewPrev), Action::Shell(ShellAction::ViewNext), Label::HintNav);
const COMMANDS: Action = Action::CommandLine(CommandLineAction::Open);
const HELP: Action = Action::Help(HelpAction::Open);
const QUIT: Action = Action::App(AppAction::Quit);
const WRITE: Action = Action::Pane(PaneAction::Insert);
const ESCAPE: Action = Action::Pane(PaneAction::Escape);
const PREV_MSG: Action = Action::Pane(PaneAction::Prev);
const NEXT_MSG: Action = Action::Pane(PaneAction::Next);
const NEW_TAB: Action = Action::Tab(TabAction::Open);

/// The entries for `place`, best first (the line drops them from the end when it is short).
#[expect(clippy::too_many_lines, reason = "a table of hints per place")]
pub fn entries(place: Place) -> &'static [Hint] {
    match place {
        Place::Welcome => &[One(QUIT, Label::HintQuit), One(HELP, Label::HintHelp), One(COMMANDS, Label::HintCommands)],
        Place::ListConversation => &[
            One(OPEN, Label::HintOpen),
            One(Action::Shell(ShellAction::ListPeek), Label::HintPeek),
            One(NEW_TAB, Label::HintNewTab),
            One(NEXT_PANE, Label::HintNextPane),
            One(COMMANDS, Label::HintCommands),
            VIEWS,
            One(HELP, Label::HintHelp),
            Leader(Label::HintMore),
        ],
        Place::ListSection => &[
            One(OPEN, Label::HintFold),
            Pair(Action::Shell(ShellAction::ListNext), Action::Shell(ShellAction::ListPrev), Label::HintMove),
            One(NEXT_PANE, Label::HintNextPane),
            VIEWS,
            One(HELP, Label::HintHelp),
            Leader(Label::HintMore),
        ],
        Place::ListEmpty => {
            &[VIEWS, One(NEXT_PANE, Label::HintNextPane), One(HELP, Label::HintHelp), Leader(Label::HintMore)]
        }
        Place::ViewSwitcher => &[
            One(Action::Shell(ShellAction::NavSelect), Label::HintShow),
            Pair(Action::Shell(ShellAction::NavNext), Action::Shell(ShellAction::NavPrev), Label::HintMove),
            One(Action::Shell(ShellAction::NavLeave), Label::HintBack),
            One(Action::Shell(ShellAction::ToggleNavRows), Label::HintFold),
            One(HELP, Label::HintHelp),
            Leader(Label::HintMore),
        ],
        Place::WorkEmpty => &[
            One(Action::Pane(PaneAction::Back), Label::HintBack),
            One(NEXT_PANE, Label::HintNextPane),
            One(NAV, Label::HintNav),
            One(HELP, Label::HintHelp),
            Leader(Label::HintMore),
        ],
        Place::Pane => &[
            One(WRITE, Label::HintWrite),
            One(PREV_MSG, Label::HintMessages),
            One(ESCAPE, Label::HintList),
            One(NAV, Label::HintNav),
            One(HELP, Label::HintHelp),
            Leader(Label::HintMore),
        ],
        Place::PaneSelected => &[
            One(Action::Pane(PaneAction::OpenThread), Label::HintThread),
            One(NEW_TAB, Label::HintNewTab),
            One(Action::Pane(PaneAction::Copy), Label::HintCopy),
            One(Action::Pane(PaneAction::Visual), Label::HintSelect),
            One(WRITE, Label::HintWrite),
            One(ESCAPE, Label::HintDeselect),
            One(HELP, Label::HintHelp),
        ],
        Place::Thread => &[
            One(WRITE, Label::HintReply),
            One(PREV_MSG, Label::HintMessages),
            One(ESCAPE, Label::HintMain),
            One(Action::Pane(PaneAction::Close), Label::HintClose),
            One(HELP, Label::HintHelp),
        ],
        Place::ThreadPane => &[
            One(WRITE, Label::HintReply),
            One(PREV_MSG, Label::HintMessages),
            One(ESCAPE, Label::HintList),
            One(Action::Pane(PaneAction::Close), Label::HintClose),
            One(HELP, Label::HintHelp),
        ],
        Place::ThreadSelected => &[
            One(WRITE, Label::HintReply),
            One(Action::Pane(PaneAction::Copy), Label::HintCopy),
            One(Action::Pane(PaneAction::Visual), Label::HintSelect),
            One(ESCAPE, Label::HintDeselect),
            One(HELP, Label::HintHelp),
        ],
        Place::Visual => &[
            One(Action::Pane(PaneAction::Copy), Label::HintCopy),
            Pair(NEXT_MSG, PREV_MSG, Label::HintExtend),
            One(ESCAPE, Label::HintCancel),
        ],
        Place::Insert => &[
            One(Action::Composer(ComposerAction::Send), Label::HintSend),
            One(Action::Composer(ComposerAction::Newline), Label::HintNewline),
            One(Action::Composer(ComposerAction::Leave), Label::HintStop),
        ],
        Place::CommandLine => &[
            One(Action::CommandLine(CommandLineAction::Run), Label::HintRun),
            One(Action::CommandLine(CommandLineAction::Cancel), Label::HintCancel),
        ],
    }
}

/// The keys shown for `action` from `ctx`: the shortest binding that works without the kitty
/// keyboard protocol (else the first one); of equals, the first in the table.
pub fn key_label(km: &Keymap, action: Action, ctx: Ctx) -> Option<String> {
    let all = km.keys_for(action, ctx);
    let plain = all.iter().filter(|k| !k.iter().any(|c| check::needs_protocol(*c))).min_by_key(|k| k.len());
    plain.or(all.first()).map(|k| keys::label(k))
}

/// The entries of `place` with their keys, typed from `ctx`: (keys, label).
pub fn resolve(km: &Keymap, place: Place, ctx: Ctx) -> Vec<(String, Label)> {
    let leader = parse_keys(LEADER).expect("the leader parses");
    entries(place)
        .iter()
        .filter_map(|h| match *h {
            One(a, l) => key_label(km, a, ctx).map(|k| (k, l)),
            Pair(a, b, l) => Some((format!("{}/{}", key_label(km, a, ctx)?, key_label(km, b, ctx)?), l)),
            Leader(l) => {
                let any = ctx.chain().iter().any(|c| km.bindings(*c).any(|(k, _)| k.len() > 1 && k[..1] == leader[..]));
                any.then(|| (keys::label(&leader), l))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_place_has_hints_whose_keys_are_bound_where_they_are_shown() {
        let km = Keymap::default();
        let cases = [
            (Place::Welcome, Ctx::Root),
            (Place::ListConversation, Ctx::List),
            (Place::ListSection, Ctx::List),
            (Place::ListEmpty, Ctx::List),
            (Place::ViewSwitcher, Ctx::ViewSwitcher),
            (Place::WorkEmpty, Ctx::PaneNormal),
            (Place::Pane, Ctx::PaneNormal),
            (Place::PaneSelected, Ctx::PaneNormal),
            (Place::Thread, Ctx::PaneNormal),
            (Place::ThreadSelected, Ctx::PaneNormal),
            (Place::ThreadPane, Ctx::PaneNormal),
            (Place::Visual, Ctx::PaneVisual),
            (Place::Insert, Ctx::ComposerInsert),
            (Place::CommandLine, Ctx::CommandLine),
        ];
        for (place, ctx) in cases {
            let got = resolve(&km, place, ctx);
            assert_eq!(got.len(), entries(place).len(), "{place:?}: every hint has a key: {got:?}");
        }
        let pane: Vec<String> = resolve(&km, Place::Pane, Ctx::PaneNormal).into_iter().map(|(k, _)| k).collect();
        assert_eq!(pane, ["i", "k", "Esc", "Ctrl+R", "?", "Space"]);
        let insert: Vec<String> =
            resolve(&km, Place::Insert, Ctx::ComposerInsert).into_iter().map(|(k, _)| k).collect();
        assert_eq!(insert, ["Enter", "Alt+Enter", "Esc"], "never a key that needs the kitty protocol");
        assert_eq!(resolve(&km, Place::Welcome, Ctx::Root)[0].0, "Ctrl+Q");
    }

    #[test]
    fn a_rebound_key_shows_as_bound_and_an_unbound_action_is_left_out() {
        let mut bound: Vec<_> = Keymap::default().all().to_vec();
        for b in &mut bound {
            if b.action == WRITE {
                b.keys = parse_keys("o").unwrap();
            }
        }
        bound.retain(|b| b.action != HELP);
        let km = Keymap::from_bound(bound);
        let got = resolve(&km, Place::Pane, Ctx::PaneNormal);
        assert_eq!(got[0], ("o".to_string(), Label::HintWrite));
        assert!(!got.iter().any(|(_, l)| *l == Label::HintHelp));
    }
}
