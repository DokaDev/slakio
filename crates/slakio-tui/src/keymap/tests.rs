use super::*;
use crate::action::AppAction;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

fn feed_all(km: &Keymap, ctx: Ctx, keys: &str) -> Vec<Resolved> {
    let mut st = KeyState::default();
    parse_keys(keys).unwrap().into_iter().map(|k| km.feed(&mut st, ctx, k)).collect()
}

#[test]
fn every_default_binding_parses_and_the_defaults_have_no_conflict() {
    for b in DEFAULTS {
        assert!(parse_keys(b.keys).is_ok(), "{}: {:?}", b.keys, parse_keys(b.keys));
    }
    let km = Keymap::default();
    for enhanced in [false, true] {
        let conflicts = check(km.all(), enhanced);
        let text: Vec<String> = conflicts.iter().map(ToString::to_string).collect();
        assert!(conflicts.is_empty(), "enhanced={enhanced}: {text:#?}");
    }
}

#[test]
fn keys_resolve_in_their_context_and_text_input_keeps_its_keys() {
    let km = Keymap::default();
    let open = Resolved::Action(Action::CommandLine(CommandLineAction::Open));
    assert_eq!(feed_all(&km, Ctx::Root, ":"), std::slice::from_ref(&open));
    assert_eq!(feed_all(&km, Ctx::List, ":"), [open], "found in a parent");
    // Typed on the command line, `:` is text.
    assert_eq!(feed_all(&km, Ctx::CommandLine, ":"), [Resolved::Unbound(parse_keys(":").unwrap())]);
    let enter = KeyChord::new(KeyCode::Enter, KeyModifiers::NONE);
    let mut st = KeyState::default();
    assert_eq!(
        km.feed(&mut st, Ctx::CommandLine, enter),
        Resolved::Action(Action::CommandLine(CommandLineAction::Run))
    );
    assert_eq!(km.feed(&mut st, Ctx::Root, enter), Resolved::Unbound(vec![enter]));
    assert_ne!(feed_all(&km, Ctx::Root, "q"), [Resolved::Action(Action::App(AppAction::Quit))], "no one-key quit");
}

#[test]
fn sequences_wait_for_their_next_key_and_drop_when_it_does_not_fit() {
    let quit = Action::App(AppAction::Quit);
    let open = Action::CommandLine(CommandLineAction::Open);
    let km = Keymap::from_bound(vec![
        Bound { ctx: Ctx::Shell, keys: parse_keys("space w q").unwrap(), action: quit },
        Bound { ctx: Ctx::List, keys: parse_keys("g g").unwrap(), action: open },
    ]);
    assert_eq!(feed_all(&km, Ctx::List, "space w q"), [Resolved::Pending, Resolved::Pending, Resolved::Action(quit)]);
    assert_eq!(feed_all(&km, Ctx::List, "g g"), [Resolved::Pending, Resolved::Action(open)]);
    assert_eq!(
        feed_all(&km, Ctx::List, "space x"),
        [Resolved::Pending, Resolved::Unbound(parse_keys("space x").unwrap())]
    );
    // A sequence started in one context does not continue in another.
    let mut st = KeyState::default();
    assert_eq!(km.feed(&mut st, Ctx::List, KeyChord::char('g')), Resolved::Pending);
    assert_eq!(st.pending(), parse_keys("g").unwrap());
    assert_eq!(km.feed(&mut st, Ctx::Rail, KeyChord::char('g')), Resolved::Unbound(parse_keys("g").unwrap()));
    assert!(st.pending().is_empty());
    assert_eq!(km.keys_for(quit, Ctx::List), [parse_keys("space w q").unwrap()]);
}

#[test]
fn key_notation_round_trips() {
    for n in [":", "enter", "esc", "ctrl+e", "shift+tab", "G", "space c n", "f4", "ctrl+[", "alt+1"] {
        let k = parse_keys(n).unwrap();
        assert_eq!(keys::notation(&k), n);
    }
    assert_eq!(keys::label(&parse_keys("ctrl+e").unwrap()), "Ctrl+E");
    assert_eq!(parse_keys("shift+g").unwrap(), parse_keys("G").unwrap(), "G and shift+g are the same key");
    assert_eq!(parse_keys(""), Err(KeyError::Empty));
    assert!(matches!(parse_keys("hyper+x"), Err(KeyError::Unknown(_))));
}

#[test]
fn every_context_has_a_unique_id_and_text_inputs_have_no_parent() {
    let ids: Vec<_> = Ctx::ALL.iter().map(|c| c.id()).collect();
    for (i, id) in ids.iter().enumerate() {
        assert!(!ids[i + 1..].contains(id), "{id}");
    }
    for c in Ctx::ALL {
        if c.is_text_input() {
            assert_eq!(c.parent(), None, "{}", c.id());
        }
        assert_eq!(c.chain().last(), Some(&if c.is_text_input() { *c } else { Ctx::Root }));
    }
}
