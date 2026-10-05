//! The checker on broken tables. Existing actions stand in for the actions of later steps: the
//! rules look at keys and contexts, not at what an action does.

use super::*;
use crate::action::{Action, AppAction, CommandLineAction};
use crate::keymap::parse_keys;

const A: Action = Action::App(AppAction::Quit);
const B: Action = Action::CommandLine(CommandLineAction::Open);
const C: Action = Action::CommandLine(CommandLineAction::Cancel);

fn b(ctx: Ctx, keys: &str, action: Action) -> Bound {
    Bound { ctx, keys: parse_keys(keys).unwrap(), action }
}

fn kinds(bindings: &[Bound], enhanced: bool) -> Vec<ConflictKind> {
    let mut k: Vec<_> = check(bindings, enhanced).into_iter().map(|c| c.kind).collect();
    k.sort();
    k.dedup();
    k
}

#[test]
fn duplicates_shadows_prefixes_and_text_keys_are_reported() {
    use ConflictKind::*;
    assert_eq!(kinds(&[b(Ctx::List, "x", A), b(Ctx::List, "x", B)], false), [Duplicate]);
    assert_eq!(kinds(&[b(Ctx::Shell, "x", A), b(Ctx::List, "x", B)], false), [Shadow]);
    assert_eq!(kinds(&[b(Ctx::Shell, "x", A), b(Ctx::List, "x", A)], false), [], "same action, nothing hidden");
    assert_eq!(kinds(&[b(Ctx::List, "g", A), b(Ctx::List, "g g", B)], false), [Prefix]);
    assert_eq!(kinds(&[b(Ctx::Shell, "space w", A), b(Ctx::Rail, "space w h", B)], false), [Prefix]);
    assert_eq!(kinds(&[b(Ctx::CommandLine, "a", A)], false), [TextKey]);
    assert_eq!(kinds(&[b(Ctx::CommandLine, "space", A)], false), [TextKey]);
    // Siblings never see each other's keys.
    assert_eq!(kinds(&[b(Ctx::Rail, "j", A), b(Ctx::List, "j", B)], false), []);
}

#[test]
fn keys_a_legacy_terminal_cannot_tell_apart_clash_unless_the_kitty_protocol_is_on() {
    for (fragile_key, plain) in [("ctrl+h", "backspace"), ("ctrl+i", "tab"), ("ctrl+m", "enter"), ("ctrl+[", "esc")] {
        // The fragile key has a fallback, so only the indistinguishable rule speaks.
        let table = [b(Ctx::Shell, fragile_key, A), b(Ctx::Shell, "space w h", A), b(Ctx::List, plain, B)];
        assert_eq!(kinds(&table, false), [ConflictKind::Indistinguishable], "{fragile_key} vs {plain}");
        assert_eq!(kinds(&table, true), [], "{fragile_key} vs {plain} with the kitty protocol");
        let c = check(&table, false).remove(0);
        assert_eq!(c.ctx, Ctx::List);
        assert!(c.to_string().contains(fragile_key) && c.to_string().contains(plain), "{c}");
    }
    // Bound to the same action, the two keys agree whatever arrives.
    let same = [b(Ctx::List, "ctrl+m", B), b(Ctx::List, "enter", B)];
    assert_eq!(kinds(&same, false), []);
    // In sequences too.
    let seq = [b(Ctx::List, "g ctrl+i", A), b(Ctx::List, "g a", A), b(Ctx::List, "g tab", B)];
    assert_eq!(kinds(&seq, false), [ConflictKind::Indistinguishable]);
}

#[test]
fn an_action_reachable_only_by_fragile_keys_needs_a_fallback() {
    use ConflictKind::NoFallback;
    assert_eq!(kinds(&[b(Ctx::Shell, "ctrl+h", A)], false), [NoFallback]);
    assert_eq!(kinds(&[b(Ctx::Shell, "ctrl+h", A)], true), [NoFallback], "the fallback is needed on every terminal");
    assert_eq!(kinds(&[b(Ctx::Shell, "alt+1", A)], false), [NoFallback]);
    assert_eq!(kinds(&[b(Ctx::Shell, "ctrl+h", A), b(Ctx::Shell, "space w h", A)], false), []);
    // A fallback in a parent counts; one in a sibling does not.
    assert_eq!(kinds(&[b(Ctx::List, "alt+1", A), b(Ctx::Shell, "space 1", A)], false), []);
    assert_eq!(kinds(&[b(Ctx::List, "alt+1", A), b(Ctx::Rail, "space 1", A)], false), [NoFallback]);
    // `Ctrl+L` / `Ctrl+J` / `Ctrl+K` arrive as themselves.
    assert_eq!(kinds(&[b(Ctx::Shell, "ctrl+l", A), b(Ctx::Shell, "ctrl+j", B), b(Ctx::Shell, "ctrl+k", C)], false), []);
}

/// `Ctrl+W` closes in Normal mode and deletes a word while typing: a text input has no parent,
/// so the two never meet.
#[test]
fn the_same_ctrl_key_in_a_text_input_and_in_normal_mode_does_not_clash() {
    let table = [b(Ctx::PaneNormal, "ctrl+w", A), b(Ctx::CommandLine, "ctrl+w", B)];
    assert_eq!(kinds(&table, false), []);
}
