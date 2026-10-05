//! What may follow an unfinished key sequence (`Space`, `Space w`, `g`): the which-key popup's
//! content, read from the key map so it always lists what the keys do now.

use super::{Ctx, KeyChord, Keymap, keys, parse_keys};
use crate::action::{self, Action};
use slakio_core::i18n::Label;

/// One key that may come next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Next {
    /// The key, as shown (`h`, `W`, `[`).
    pub key: String,
    pub what: What,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum What {
    /// The key ends the sequence and runs this action.
    Action(Action),
    /// More keys follow (`Space w` → `+Window`): the group's name, if it has one.
    Group(Option<Label>),
}

/// Names of the groups of keys (the sequence typed so far, in config notation).
pub const GROUPS: &[(&str, Label)] =
    &[("space", Label::GuideLeader), ("space w", Label::GuideWindow), ("g", Label::GuideGo)];

/// The name of the group `pending` opens, if it has one.
pub fn group(pending: &[KeyChord]) -> Option<Label> {
    GROUPS.iter().find(|(n, _)| parse_keys(n).ok().as_deref() == Some(pending)).map(|(_, l)| *l)
}

/// The keys that may follow `pending` typed in `ctx`, in the order of the table, groups last (an
/// inner context's binding hides a parent's for the same key).
pub fn next(km: &Keymap, ctx: Ctx, pending: &[KeyChord]) -> Vec<Next> {
    let mut out: Vec<(KeyChord, Next)> = Vec::new();
    for c in ctx.chain() {
        for (k, a) in km.bindings(c) {
            if k.len() <= pending.len() || !k.starts_with(pending) {
                continue;
            }
            let key = k[pending.len()];
            if out.iter().any(|(seen, _)| *seen == key) {
                continue;
            }
            let what = if k.len() == pending.len() + 1 {
                What::Action(a)
            } else {
                let mut deeper = pending.to_vec();
                deeper.push(key);
                What::Group(group(&deeper))
            };
            out.push((key, Next { key: keys::label(&[key]), what }));
        }
    }
    // Groups after the keys that run something.
    let (groups, actions): (Vec<Next>, Vec<Next>) =
        out.into_iter().map(|(_, n)| n).partition(|n| matches!(n.what, What::Group(_)));
    actions.into_iter().chain(groups).collect()
}

/// The label of what `n` does.
pub fn label(n: &Next) -> Label {
    match n.what {
        What::Action(a) => action::spec(a).label,
        What::Group(Some(l)) => l,
        What::Group(None) => Label::GuideMore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{HelpAction, ShellAction};
    use crate::app::shell::View;

    #[test]
    fn the_leader_lists_its_keys_and_groups() {
        let km = Keymap::default();
        let space = parse_keys("space").unwrap();
        let n = next(&km, Ctx::List, &space);
        let h = n.iter().find(|n| n.key == "h").expect("Space h");
        assert_eq!(h.what, What::Action(Action::Shell(ShellAction::Show(View::Home))));
        let w = n.iter().find(|n| n.key == "w").expect("Space w");
        assert_eq!(w.what, What::Group(Some(Label::GuideWindow)));
        assert!(n.iter().any(|n| n.key == "?" && n.what == What::Action(Action::Help(HelpAction::Open))));
        assert_eq!(n.iter().filter(|n| n.key == "w").count(), 1, "a group is listed once");
        let window = next(&km, Ctx::List, &parse_keys("space w").unwrap());
        assert!(["h", "j", "k", "l", "c"].iter().all(|k| window.iter().any(|n| n.key == *k)), "{window:?}");
        assert_eq!(group(&space), Some(Label::GuideLeader));
        assert!(next(&km, Ctx::ComposerInsert, &space).is_empty(), "nothing follows Space while typing");
    }
}
