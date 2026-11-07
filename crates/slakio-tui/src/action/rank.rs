//! Matching typed words to actions: the palette's and the key help's search.

use super::Action;
use super::{ActionSpec, REGISTRY};

/// Fuzzy subsequence score of `query` in `text` (case and spaces in `query` ignored); `None`
/// when `query` is not a subsequence of `text`. The first character of `query` must start a
/// word of `text`, so a short word does not pick an action that merely holds its letters.
/// Higher is better: bonuses for word starts and runs, a cost for gaps and a late start.
pub fn word_score(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let boundary = |i: usize| i == 0 || matches!(t[i - 1], ' ' | '.' | '_' | '-' | '/' | '(' | ':');
    let mut best: Option<i32> = None;
    // Every start of the first query character at a word start; greedy after it.
    for start in (0..t.len()).filter(|&i| t[i] == q[0] && boundary(i)) {
        let (mut score, mut qi, mut prev) = (0i32, 0, None::<usize>);
        for (i, &c) in t.iter().enumerate().skip(start) {
            if qi < q.len() && c == q[qi] {
                score += 1 + if boundary(i) { 8 } else { 0 };
                score += match prev {
                    Some(p) if p + 1 == i => 5,
                    Some(p) => -((i - p - 1) as i32).min(3),
                    None => -(start as i32).min(5),
                };
                prev = Some(i);
                qi += 1;
            }
        }
        if qi == q.len() {
            best = Some(best.map_or(score, |b| b.max(score)));
        }
    }
    best
}

/// How well `query` matches an entry typed as `names` (`:` command names) and described by
/// `words` (its labels and id): `(tier, score)`, the lower tier first, then the higher score;
/// `None` when it does not match. Case is ignored.
///
/// | tier | match | e.g. |
/// |---|---|---|
/// | 0 | a name is `query` | `q` for `:q` |
/// | 1 | a name starts with it | `the` for `:theme` |
/// | 2 | a word of a name or of `words` starts with it | `color` for "Change the color theme" |
/// | 3 | its letters in order, the first starting a word ([`word_score`]) | `thm` for `:theme` |
///
/// An empty `query` matches everything equally (tier 0).
pub fn rank(query: &str, names: &[&str], words: &[&str]) -> Option<(u8, i32)> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some((0, 0));
    }
    let all = || names.iter().chain(words);
    if names.iter().any(|n| n.to_lowercase() == q) {
        return Some((0, 0));
    }
    if let Some(n) = names.iter().filter(|n| n.to_lowercase().starts_with(&q)).map(|n| n.chars().count()).min() {
        // The shortest name it starts first (`:q` before `:quit`).
        return Some((1, -(n as i32)));
    }
    let score = all().filter_map(|t| word_score(&q, t)).max()?;
    let word_start = all().any(|t| {
        let t = t.to_lowercase();
        t.match_indices(q.as_str()).any(|(i, _)| {
            t[..i].chars().next_back().is_none_or(|c| matches!(c, ' ' | '.' | '_' | '-' | '/' | '(' | ':'))
        })
    });
    Some((if word_start { 2 } else { 3 }, score))
}

/// How well `query` matches the registered action `s` ([`rank`]): its command names, its label
/// in the UI language and in English, and its id. English words find actions while the UI is
/// in Korean too.
pub fn rank_spec(query: &str, s: &ActionSpec, i18n: &slakio_core::i18n::I18n) -> Option<(u8, i32)> {
    let english = slakio_core::i18n::I18n::new(slakio_core::i18n::Lang::En);
    let (own, en) = (i18n.label(s.label).to_string(), english.label(s.label).to_string());
    rank(query, s.commands, &[own.as_str(), en.as_str(), s.id])
}

/// Registry indices that match `query` ([`rank_spec`]), best first; ties keep the registry's
/// order.
pub fn search(query: &str, i18n: &slakio_core::i18n::I18n) -> Vec<usize> {
    let mut hits: Vec<((u8, i32), usize)> = REGISTRY
        .iter()
        .enumerate()
        .filter_map(|(i, s)| rank_spec(query, s, i18n).map(|(t, sc)| ((t, -sc), i)))
        .collect();
    hits.sort_unstable();
    hits.into_iter().map(|(_, i)| i).collect()
}

/// The action a `:` command names (surrounding spaces ignored), if any.
pub fn by_command(name: &str) -> Option<Action> {
    let name = name.trim();
    REGISTRY.iter().find(|s| s.commands.contains(&name)).map(|s| s.action)
}
