//! What may follow an unfinished key sequence (`Space`, `Space w`, `g`): the which-key popup's
//! content, read from the key map so it always lists what the keys do now.

use super::{Ctx, KeyChord, Keymap, keys, parse_keys};
use crate::action::{self, Action, TabAction};
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
    /// Keys that show a tab by its number, listed as one entry (`1…9`).
    TabNumbers,
}

/// Names of the groups of keys (the sequence typed so far, in config notation).
pub const GROUPS: &[(&str, Label)] = &[
    ("space", Label::GuideLeader),
    ("space w", Label::GuideWindow),
    ("space t", Label::GuideTab),
    ("g", Label::GuideGo),
];

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
    // The tab numbers are one entry, `1…9`, where the first of them is.
    let number = |n: &Next| matches!(n.what, What::Action(Action::Tab(TabAction::Go(_))));
    let numbers: Vec<String> = out.iter().filter(|(_, n)| number(n)).map(|(_, n)| n.key.clone()).collect();
    if let [first, .., last] = &numbers[..] {
        let at = out.iter().position(|(_, n)| number(n)).unwrap_or(0);
        out[at].1 = Next { key: format!("{first}…{last}"), what: What::TabNumbers };
        out.retain(|(_, n)| !number(n));
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
        What::TabNumbers => Label::GuideTabNumbers,
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
        let numbers: Vec<&Next> = n.iter().filter(|n| n.key.starts_with('1')).collect();
        assert_eq!(numbers.len(), 1, "the tab numbers are one entry: {n:?}");
        assert_eq!((numbers[0].key.as_str(), numbers[0].what), ("1…9", What::TabNumbers));
        assert!(!n.iter().any(|n| n.key == "5"));
        assert!(n.iter().any(|n| n.key == "t" && n.what == What::Group(Some(Label::GuideTab))));
    }
}
