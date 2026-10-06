//! The key conflict checker. Seen from every context (its own bindings and its parents'), the
//! bindings must not:
//!
//! 1. bind the same keys twice in one context ([`ConflictKind::Duplicate`]);
//! 2. hide a parent's keys with another action ([`ConflictKind::Shadow`]);
//! 3. make one sequence a prefix of another, e.g. `g` and `g g` ([`ConflictKind::Prefix`]);
//! 4. bind a character key or a `Space` sequence in a text input ([`ConflictKind::TextKey`]);
//! 5. bind, to different actions, keys the terminal cannot tell apart without the kitty keyboard
//!    protocol ([`ConflictKind::Indistinguishable`]): `Ctrl+I` arrives as `Tab`, `Ctrl+M` as
//!    `Enter`, `Ctrl+[` as `Esc`, and `Backspace` as `Ctrl+H` on terminals that send `^H`.
//!    Checked unless the protocol is active (`enhanced`);
//! 6. leave an action reachable only by fragile keys ([`ConflictKind::NoFallback`]): one of the
//!    `Ctrl` keys above (they need the kitty protocol, or a terminal that sends a code of its
//!    own) or an `Alt` key (macOS terminals send it only with "Option as Alt"). Every such
//!    action also needs a binding that works everywhere, e.g. `Space w h` next to `Ctrl+H`;
//! 7. take a global key ([`Ctx::Global`]: quit, help, the command palette), which is looked up
//!    before every context, for another action or as the start of a sequence
//!    ([`ConflictKind::Protected`]): the binding could never be reached.

use super::{Bound, Ctx, KeyChord, keys};
use crate::action;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConflictKind {
    Duplicate,
    Shadow,
    Prefix,
    TextKey,
    Indistinguishable,
    NoFallback,
    Protected,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub kind: ConflictKind,
    /// The context the clash is seen from.
    pub ctx: Ctx,
    pub a: Bound,
    /// The other binding (`None` for a problem of one binding).
    pub b: Option<Bound>,
}

fn describe(b: &Bound) -> String {
    format!("[{}] {} = {}", b.ctx.id(), keys::notation(&b.keys), action::spec(b.action).id)
}

/// Developer text (tests, and later the startup check of user bindings).
impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} in {}: {}", self.kind, self.ctx.id(), describe(&self.a))?;
        if let Some(b) = &self.b {
            write!(f, " vs {}", describe(b))?;
        }
        Ok(())
    }
}

/// The key the terminal sends for `k` without the kitty protocol, when that is another key.
pub fn legacy(k: KeyChord) -> KeyChord {
    if k.mods != KeyModifiers::CONTROL {
        return k;
    }
    let code = match k.code {
        KeyCode::Char('i') => KeyCode::Tab,
        KeyCode::Char('m') => KeyCode::Enter,
        KeyCode::Char('[') => KeyCode::Esc,
        KeyCode::Char('h') => KeyCode::Backspace,
        _ => return k,
    };
    KeyChord::new(code, KeyModifiers::NONE)
}

/// A key a terminal sends as itself only with the kitty keyboard protocol (`Ctrl+I` arrives as
/// `Tab`, `Shift+Enter` as `Enter`): never the one a hint shows when another key does it.
pub fn needs_protocol(k: KeyChord) -> bool {
    legacy(k) != k || (k.code == KeyCode::Enter && k.mods.contains(KeyModifiers::SHIFT))
}

/// A key that some terminals cannot send as itself: see rule 6.
pub fn fragile(k: KeyChord) -> bool {
    legacy(k) != k || k.mods.contains(KeyModifiers::ALT)
}

fn folded(keys: &[KeyChord]) -> Vec<KeyChord> {
    keys.iter().map(|k| legacy(*k)).collect()
}

fn same_or_prefix(a: &[KeyChord], b: &[KeyChord]) -> (bool, bool) {
    let same = a == b;
    (same, !same && (a.starts_with(b) || b.starts_with(a)))
}

/// Conflicts among `bindings`, seen from every context. `enhanced`: the kitty keyboard protocol
/// is active, so the keys of rule 5 are told apart.
pub fn check(bindings: &[Bound], enhanced: bool) -> Vec<Conflict> {
    let mut out: Vec<Conflict> = Vec::new();
    let mut push = |c: Conflict| {
        let seen = out.iter().any(|o| {
            o.kind == c.kind
                && ((o.a == c.a && o.b == c.b) || (Some(&o.a) == c.b.as_ref() && o.b.as_ref() == Some(&c.a)))
        });
        if !seen {
            out.push(c);
        }
    };
    for g in bindings.iter().filter(|b| b.ctx == Ctx::Global) {
        for b in bindings.iter().filter(|b| b.ctx != Ctx::Global && b.keys[0] == g.keys[0]) {
            if b.keys.len() > 1 || b.action != g.action {
                push(Conflict { kind: ConflictKind::Protected, ctx: b.ctx, a: b.clone(), b: Some(g.clone()) });
            }
        }
    }
    for &leaf in Ctx::ALL {
        let chain = leaf.chain();
        let depth = |c: Ctx| chain.iter().position(|x| *x == c);
        let eff: Vec<(&Bound, usize)> = bindings.iter().filter_map(|b| depth(b.ctx).map(|d| (b, d))).collect();
        for (i, &(x, dx)) in eff.iter().enumerate() {
            if dx == 0
                && x.ctx.is_text_input()
                && (x.keys[0] == KeyChord::char(' ') || x.keys.iter().any(KeyChord::is_plain_char))
            {
                push(Conflict { kind: ConflictKind::TextKey, ctx: leaf, a: x.clone(), b: None });
            }
            for &(y, dy) in &eff[i + 1..] {
                let (inner, outer) = if dx <= dy { (x, y) } else { (y, x) };
                let kind = match same_or_prefix(&x.keys, &y.keys) {
                    (true, _) if dx == dy => Some(ConflictKind::Duplicate),
                    (true, _) if x.action != y.action => Some(ConflictKind::Shadow),
                    (true, _) => None,
                    (_, true) => Some(ConflictKind::Prefix),
                    _ if enhanced || x.action == y.action => None,
                    _ => {
                        let (same, prefix) = same_or_prefix(&folded(&x.keys), &folded(&y.keys));
                        (same || prefix).then_some(ConflictKind::Indistinguishable)
                    }
                };
                if let Some(kind) = kind {
                    push(Conflict { kind, ctx: leaf, a: inner.clone(), b: Some(outer.clone()) });
                }
            }
        }
        for &(x, _) in &eff {
            let all_fragile =
                eff.iter().filter(|(y, _)| y.action == x.action).all(|(y, _)| y.keys.iter().any(|k| fragile(*k)));
            if all_fragile {
                push(Conflict { kind: ConflictKind::NoFallback, ctx: leaf, a: x.clone(), b: None });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
