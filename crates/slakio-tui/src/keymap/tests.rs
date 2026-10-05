use super::*;
use crate::action::AppAction;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

#[test]
fn every_default_binding_parses_and_no_key_is_bound_twice_in_a_context() {
    for b in DEFAULTS {
        assert!(parse_keys(b.keys).is_ok(), "{}: {:?}", b.keys, parse_keys(b.keys));
    }
    let km = Keymap::default();
    for &ctx in Ctx::ALL {
        let keys: Vec<_> = km.bindings(ctx).map(|(k, _)| k.to_vec()).collect();
        for (i, k) in keys.iter().enumerate() {
            assert!(!keys[i + 1..].contains(k), "{} bound twice in {}", keys::label(k), ctx.id());
        }
    }
}

#[test]
fn keys_resolve_in_their_context_and_text_input_keeps_its_keys() {
    let km = Keymap::default();
    let colon = KeyChord::char(':');
    assert_eq!(km.resolve(Ctx::Root, colon), Some(Action::CommandLine(CommandLineAction::Open)));
    // Typed on the command line, `:` is text.
    assert_eq!(km.resolve(Ctx::CommandLine, colon), None);
    let enter = KeyChord::new(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(km.resolve(Ctx::CommandLine, enter), Some(Action::CommandLine(CommandLineAction::Run)));
    assert_eq!(km.resolve(Ctx::Root, enter), None);
    assert_ne!(km.resolve(Ctx::Root, KeyChord::char('q')), Some(Action::App(AppAction::Quit)), "no one-key quit");
}

#[test]
fn key_notation_round_trips() {
    for n in [":", "enter", "esc", "ctrl+e", "shift+tab", "G", "space c n", "f4"] {
        let k = parse_keys(n).unwrap();
        assert_eq!(keys::notation(&k), n);
    }
    assert_eq!(keys::label(&parse_keys("ctrl+e").unwrap()), "Ctrl+E");
    assert_eq!(parse_keys("shift+g").unwrap(), parse_keys("G").unwrap(), "G and shift+g are the same key");
    assert_eq!(parse_keys(""), Err(KeyError::Empty));
    assert!(matches!(parse_keys("hyper+x"), Err(KeyError::Unknown(_))));
}

#[test]
fn every_context_has_a_unique_id() {
    let ids: Vec<_> = Ctx::ALL.iter().map(|c| c.id()).collect();
    for (i, id) in ids.iter().enumerate() {
        assert!(!ids[i + 1..].contains(id), "{id}");
    }
}
