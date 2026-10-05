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
fn every_action_is_registered() {
    use ShellAction::*;
    let shell = [
        FocusLeft, FocusRight, RailNext, RailPrev, RailSelect, ListNext, ListPrev, ListFirst, ListLast, ListOpen,
        ToggleList,
    ]
    .into_iter()
    .chain(View::ALL.iter().map(|v| Show(*v)))
    .map(Action::Shell);
    let pane = {
        use PaneAction::*;
        [Next, Prev, First, Last, OpenThread, Visual, Copy, Escape, Insert, Close, Back, Forward].map(Action::Pane)
    };
    let composer = {
        use ComposerAction::*;
        [Send, Newline, Leave, DeleteWord, DeleteLine].map(Action::Composer)
    };
    let all = [
        Action::App(AppAction::Quit),
        Action::CommandLine(CommandLineAction::Open),
        Action::CommandLine(CommandLineAction::Run),
        Action::CommandLine(CommandLineAction::Cancel),
    ]
    .into_iter()
    .chain(shell)
    .chain(pane)
    .chain(composer);
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
    assert_eq!(by_command(" q "), Some(Action::App(AppAction::Quit)));
    assert_eq!(by_command("dms"), Some(Action::Shell(ShellAction::Show(View::Dms))));
    assert_eq!(by_command("wq"), None);
    assert_eq!(by_command(""), None);
}
