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
    for a in [
        Action::App(AppAction::Quit),
        Action::CommandLine(CommandLineAction::Open),
        Action::CommandLine(CommandLineAction::Run),
        Action::CommandLine(CommandLineAction::Cancel),
    ] {
        assert_eq!(spec(a).action, a);
    }
}

#[test]
fn commands_find_their_action() {
    assert_eq!(by_command("qa"), Some(Action::App(AppAction::Quit)));
    assert_eq!(by_command(" q "), Some(Action::App(AppAction::Quit)));
    assert_eq!(by_command("wq"), None);
    assert_eq!(by_command(""), None);
}
