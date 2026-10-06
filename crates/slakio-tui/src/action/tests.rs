use super::*;
use std::collections::HashSet;

#[test]
fn ids_and_command_names_are_unique() {
    let mut ids = HashSet::new();
    let mut commands = HashSet::new();
    for s in REGISTRY {
        assert!(ids.insert(s.id), "id {} twice", s.id);
        for c in s.commands {
            assert!(commands.insert(*c), "command {c} twice");
        }
    }
}

#[test]
#[expect(clippy::too_many_lines, reason = "lists every action")]
fn every_action_is_registered() {
    let shell = {
        use ShellAction::*;
        [
            FocusLeft,
            FocusRight,
            FocusUp,
            FocusDown,
            FocusNext,
            FocusPrev,
            FocusNav,
            NavNext,
            NavPrev,
            NavFirst,
            NavLast,
            NavSelect,
            NavLeave,
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
            ListOpen,
            ListPeek,
            ListLeft,
            ListSectionPrev,
            ListSectionNext,
            ToggleList,
        ]
        .into_iter()
        .chain(View::ALL.iter().map(|v| Show(*v)))
        .map(Action::Shell)
    };
    let pane = {
        use PaneAction::*;
        [
            Next, Prev, First, Last, HalfDown, HalfUp, PageDown, PageUp, OpenThread, Visual, Copy, Escape, Insert,
            Close, Left, Right, Back, Forward,
        ]
        .map(Action::Pane)
    };
    let tab = {
        use TabAction::*;
        [Open, Next, Prev, Close, Reopen, Rename, MoveLeft, MoveRight]
            .into_iter()
            .chain((1..=9).map(Go))
            .map(Action::Tab)
    };
    let composer = {
        use ComposerAction::*;
        [Send, Newline, Leave, DeleteWord, DeleteLine, DeleteToEnd].map(Action::Composer)
    };
    let help = {
        use HelpAction::*;
        [
            Open,
            Close,
            Next,
            Prev,
            PageDown,
            PageUp,
            First,
            Last,
            Run,
            Expand,
            Collapse,
            Search,
            SearchDone,
            SearchCancel,
        ]
        .map(Action::Help)
    };
    let dialog = {
        use DialogAction::*;
        [Yes, No, Choose, Toggle].map(Action::Dialog)
    };
    let app = {
        use AppAction::*;
        [Quit, Interrupt, Palette, ChooseWorkspace, ToggleAvatars, ToggleIcons, ToggleDensity].map(Action::App)
    };
    let all = app
        .into_iter()
        .chain(
            [
                CommandLineAction::Open,
                CommandLineAction::Run,
                CommandLineAction::Cancel,
                CommandLineAction::Next,
                CommandLineAction::Prev,
            ]
            .map(Action::CommandLine),
        )
        .chain(shell)
        .chain(pane)
        .chain(tab)
        .chain(composer)
        .chain(help)
        .chain(dialog);
    let mut n = 0;
    for a in all {
        assert_eq!(spec(a).action, a);
        n += 1;
    }
    assert_eq!(n, REGISTRY.len(), "every registry entry is an action listed here");
}

#[test]
fn commands_find_their_action() {
    assert_eq!(by_command("qa"), Some(Action::App(AppAction::Quit)));
    assert_eq!(by_command(" q "), Some(Action::Pane(PaneAction::Close)), ":q closes, as in vim");
    assert_eq!(by_command("dms"), Some(Action::Shell(ShellAction::Show(View::Dms))));
    assert_eq!(by_command("wq"), None);
    assert_eq!(by_command(""), None);
}

#[test]
fn search_finds_actions_by_their_words_from_word_starts() {
    use slakio_core::i18n::{I18n, Lang};
    let en = I18n::new(Lang::En);
    let first = |q: &str| search(q, &en).first().map(|&i| REGISTRY[i].id);
    assert_eq!(first("dms"), Some("view.dms"));
    assert_eq!(first("rail"), Some("nav.focus"));
    assert_eq!(first("help"), Some("help.open"));
    assert!(word_score("wq", "Pick another workspace").is_none(), "q is not there");
    assert!(word_score("ot", "Show the rail item").is_none(), "o must start a word");
    assert!(word_score("sd", "Show DMs") > word_score("sd", "Show the rail item and DMs"));
    // English words find actions in Korean too.
    let ko = I18n::new(Lang::Ko);
    assert_eq!(search("dms", &ko).first().map(|&i| REGISTRY[i].id), Some("view.dms"));
}

#[test]
fn rank_puts_names_before_word_starts_before_letters_in_order() {
    let theme = |q: &str| rank(q, &["theme", "colorscheme"], &["Change the color theme", "theme.set"]);
    assert_eq!(theme("theme"), Some((0, 0)), "a name");
    assert_eq!(theme("THE").map(|r| r.0), Some(1), "a name it starts, case ignored");
    assert_eq!(theme("color").map(|r| r.0), Some(1), "colorscheme");
    assert_eq!(theme("chan").map(|r| r.0), Some(2), "a word of the label");
    assert_eq!(theme("set").map(|r| r.0), Some(2), "a word of the id");
    assert_eq!(theme("thm").map(|r| r.0), Some(3), "its letters in order");
    assert_eq!(theme("hm"), None, "the first letter must start a word");
    assert_eq!(theme("tz"), None);
    assert_eq!(theme(""), Some((0, 0)), "nothing typed: everything");
    // The shorter name a word starts comes first (`:q` before `:quit`).
    assert!(rank("q", &["qa", "q"], &[]) < rank("q", &["quit"], &[]));
    // Within a tier, the better run of letters first.
    assert!(rank("sd", &[], &["Show DMs"]).unwrap().1 > rank("sd", &[], &["Show the rail item and DMs"]).unwrap().1);
}
