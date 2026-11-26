//! Probes of what the start bundle says of the team and of other workspaces' people, and of
//! what a page's reactions and rich-text emoji carry: field names only (each through
//! [`super::key`]) and counts, never a value, a name, an id or an address. They tell, from the
//! owner's own run, whether Slack gives an address for another workspace's emoji and where a
//! team's picture is.

use super::{KEYS_MAX, key, key_lines, type_of};
use serde_json::{Map, Value};
use slakio_core::diag::{Diag, Kind, Stage};
use std::collections::BTreeMap;

/// How deep the objects under a team-named field are looked into for picture fields.
const ICON_DEPTH: usize = 3;

/// Words that make a person's field name worth telling for one of another workspace.
const TEAMISH: [&str; 6] = ["team", "enterprise", "stranger", "external", "shared", "connect"];

/// The start bundle `v`: its `team`'s field names and those of `team.icon`; every top-level
/// field whose name has `team` in it (its type and size); the picture fields (`icon…`,
/// `image_…`) in the objects under those; and its people of another team than `team.id`.
pub(super) fn team(diag: &Diag, v: &Value) {
    let team = v.get("team");
    for k in keys(team) {
        diag.line(Kind::TeamShape, Stage::Field, "team_key", &[("name", key(k)), ("entries", "1")]);
    }
    for k in keys(team.and_then(|t| t.get("icon"))) {
        diag.line(Kind::TeamShape, Stage::Field, "team_icon_key", &[("name", key(k)), ("entries", "1")]);
    }
    let Some(o) = v.as_object() else { return };
    let teamish: Vec<(&String, &Value)> = o.iter().filter(|(k, _)| k.contains("team")).take(KEYS_MAX).collect();
    for (k, f) in &teamish {
        let count = f.as_array().map_or_else(|| "-".to_string(), |a| a.len().to_string());
        diag.line(
            Kind::TeamShape,
            Stage::Field,
            "boot_team_key",
            &[("name", key(k)), ("type", type_of(f)), ("count", &count)],
        );
    }
    for (k, f) in &teamish {
        let mut found: BTreeMap<&str, usize> = BTreeMap::new();
        icons(f, 0, &mut found);
        for (name, n) in found.into_iter().take(KEYS_MAX) {
            let n = n.to_string();
            diag.line(
                Kind::TeamShape,
                Stage::Counted,
                "icon_fields",
                &[("where", key(k)), ("name", name), ("count", &n)],
            );
        }
    }
    let own = team.and_then(|t| t.get("id")).and_then(Value::as_str);
    people(diag, v.get("users").and_then(Value::as_array).into_iter().flatten(), own);
}

/// The field names of `o`, when it is an object, at most [`KEYS_MAX`].
fn keys(o: Option<&Value>) -> impl Iterator<Item = &String> {
    o.and_then(Value::as_object).into_iter().flat_map(Map::keys).take(KEYS_MAX)
}

/// The picture fields (`icon…`, `image_…`) of `v` and the objects in it, to [`ICON_DEPTH`].
fn icons<'a>(v: &'a Value, depth: usize, found: &mut BTreeMap<&'a str, usize>) {
    if depth > ICON_DEPTH {
        return;
    }
    match v {
        Value::Object(o) => {
            for (k, f) in o.iter().take(KEYS_MAX) {
                if k.starts_with("icon") || k.starts_with("image_") {
                    *found.entry(key(k)).or_default() += 1;
                }
                icons(f, depth + 1, found);
            }
        }
        Value::Array(a) => a.iter().take(super::ENTRIES_MAX).for_each(|f| icons(f, depth + 1, found)),
        _ => {}
    }
}

/// The people of `entries` of another team than `own` (a `team_id` that is not it): how many,
/// and the team-ish field names they and their profiles have.
pub(super) fn people<'a>(diag: &Diag, entries: impl Iterator<Item = &'a Value>, own: Option<&str>) {
    let Some(own) = own else { return };
    let others: Vec<&Value> = entries
        .take(super::ENTRIES_MAX)
        .filter(|p| p.get("team_id").and_then(Value::as_str).is_some_and(|t| !t.is_empty() && t != own))
        .collect();
    diag.line(Kind::TeamShape, Stage::Counted, "external_users", &[("count", &others.len().to_string())]);
    let teamish = |k: &str, words: &[&str]| words.iter().any(|w| k.contains(w));
    names(diag, "external_user_key", others.iter().copied(), |k| teamish(k, &TEAMISH));
    names(diag, "external_profile_key", others.iter().filter_map(|p| p.get("profile")), |k| teamish(k, &["team"]));
}

/// One `field <label>` line per field name `keep` takes that the objects of `entries` have.
fn names<'a>(diag: &Diag, label: &str, entries: impl Iterator<Item = &'a Value>, keep: impl Fn(&str) -> bool) {
    let mut found: BTreeMap<&str, usize> = BTreeMap::new();
    for e in entries {
        for k in e.as_object().into_iter().flat_map(Map::keys).take(KEYS_MAX).filter(|k| keep(k)) {
            *found.entry(key(k)).or_default() += 1;
        }
    }
    for (name, n) in found.into_iter().take(KEYS_MAX) {
        diag.line(Kind::TeamShape, Stage::Field, label, &[("name", name), ("entries", &n.to_string())]);
    }
}

/// A page's messages (`page`: `history`, `replies` or `view`): the field names of their
/// reactions and of their rich-text emoji, and how many of each carry a `url`.
pub(super) fn reactions(diag: &Diag, messages: &[Value], page: &str) {
    let reactions: Vec<&Value> = (messages.iter().take(super::ENTRIES_MAX))
        .filter_map(|m| m.get("reactions").and_then(Value::as_array))
        .flatten()
        .collect();
    let mut emoji: Vec<&Value> = Vec::new();
    for m in messages.iter().take(super::ENTRIES_MAX) {
        elements(m.get("blocks"), 0, &mut emoji);
    }
    key_lines(diag, Kind::ReactionShape, "reaction_key", reactions.iter().copied(), page);
    key_lines(diag, Kind::ReactionShape, "emoji_element_key", emoji.iter().copied(), page);
    for (label, of) in [("url_on_reaction", &reactions), ("url_on_emoji_element", &emoji)] {
        let n = of.iter().filter(|e| e.get("url").is_some()).count().to_string();
        diag.line(Kind::ReactionShape, Stage::Counted, label, &[("count", &n), ("page", page)]);
    }
}

/// The rich-text emoji elements (`{"type": "emoji", …}`) in `v`, to the blocks' depth.
fn elements<'a>(v: Option<&'a Value>, depth: usize, out: &mut Vec<&'a Value>) {
    if depth > 8 || out.len() >= super::ENTRIES_MAX {
        return;
    }
    match v {
        Some(Value::Array(a)) => a.iter().for_each(|e| elements(Some(e), depth + 1, out)),
        Some(o @ Value::Object(_)) => {
            if o.get("type").and_then(Value::as_str) == Some("emoji") {
                out.push(o);
            }
            elements(o.get("elements"), depth + 1, out);
        }
        _ => {}
    }
}
