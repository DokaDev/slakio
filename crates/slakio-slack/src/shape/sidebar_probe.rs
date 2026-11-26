//! Probes of what the start bundle says of the sidebar: which conversations are starred (of what
//! kind, open or not, and how Slack says they are starred), how its channels are shared, and the
//! form of its group DMs' names. Flags and counts only, tallied: never a name, an id or a value.
//! They tell, from the owner's own run, whether Starred and the shared-channel marks read the
//! right fields.

use crate::wire;
use serde_json::Value;
use slakio_core::diag::{Diag, Kind, Stage};
use slakio_core::model::User;
use std::collections::{BTreeMap, HashSet};

/// The most lines one tally tells.
const LINES_MAX: usize = 32;

/// `y` / `n`.
fn yn(b: bool) -> &'static str {
    if b { "y" } else { "n" }
}

/// A flag of `v` named `name`: `y`, `n`, or `-` when absent or not a yes/no.
fn flag(v: &Value, name: &str) -> &'static str {
    v.get(name).and_then(Value::as_bool).map_or("-", yn)
}

/// The length of a list of `v` named `name`: `0` to `8`, `9+`, or `-` when absent or not a list.
fn size(v: &Value, name: &str) -> String {
    match v.get(name).and_then(Value::as_array).map(Vec::len) {
        Some(n) if n > 8 => "9+".to_string(),
        Some(n) => n.to_string(),
        None => "-".to_string(),
    }
}

/// One line of `label` per distinct tuple of `tally` (sorted, at most [`LINES_MAX`]): its pairs,
/// then `n`, how many had it.
fn tally_lines<const N: usize>(diag: &Diag, label: &str, names: [&str; N], tally: BTreeMap<[String; N], usize>) {
    for (values, n) in tally.into_iter().take(LINES_MAX) {
        let n = n.to_string();
        let mut pairs: Vec<(&str, &str)> = names.iter().copied().zip(values.iter().map(String::as_str)).collect();
        pairs.push(("n", &n));
        diag.line(Kind::BootShape, Stage::Field, label, &pairs);
    }
}

/// The starred conversations of `boot` (`known`: the people looked up; `me`: the user): of each,
/// its kind (`channel`, `mpim`, `im`, `app`: a DM with a bot, `unknown`: starred but in neither
/// list), whether it is the user's own DM (`self`), whether Slack says a DM is open (`open`: `-`
/// for the others or when not said), whether it is in the start bundle's `is_open` list
/// (`listed_open`) and how it is said to be starred (`source`: `list`, the start bundle's
/// `starred`; `flag`, its own `is_starred`; `both`). Then how many are starred.
pub(crate) fn starred(diag: &Diag, boot: &wire::UserBoot, known: &[User], me: &str) {
    let list: HashSet<&str> = boot.starred.iter().flatten().map(String::as_str).collect();
    let open: HashSet<&str> = boot.is_open.iter().flatten().map(String::as_str).collect();
    let source = |id: &str, flag: bool| match (list.contains(id), flag) {
        (true, true) => Some("both"),
        (true, false) => Some("list"),
        (false, true) => Some("flag"),
        (false, false) => None,
    };
    let mut tally: BTreeMap<[String; 5], usize> = BTreeMap::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut add = |values: [&str; 5]| *tally.entry(values.map(str::to_string)).or_default() += 1;
    for c in boot.channels.iter().flatten() {
        seen.insert(&c.id);
        let Some(source) = source(&c.id, c.is_starred == Some(true)) else { continue };
        let kind = if c.is_mpim == Some(true) { "mpim" } else { "channel" };
        add([kind, "n", "-", yn(open.contains(c.id.as_str())), source]);
    }
    for im in boot.ims.iter().flatten() {
        seen.insert(&im.id);
        let Some(source) = source(&im.id, im.is_starred == Some(true)) else { continue };
        let user = im.user.as_deref();
        let bot = user.is_some_and(|u| known.iter().any(|k| k.id.as_str() == u && k.bot));
        let is_open = im.is_open.map_or("-", yn);
        add([if bot { "app" } else { "im" }, yn(user == Some(me)), is_open, yn(open.contains(im.id.as_str())), source]);
    }
    for id in list.iter().filter(|id| !seen.contains(*id)) {
        add(["unknown", "n", "-", yn(open.contains(id)), "list"]);
    }
    let count = tally.values().sum::<usize>().to_string();
    tally_lines(diag, "starred", ["kind", "self", "open", "listed_open", "source"], tally);
    diag.line(Kind::BootShape, Stage::Field, "starred-total", &[("count", &count)]);
}

/// The start bundle's `channels` (its raw entries): of each channel not archived (group DMs left
/// out), whether Slack says it is shared with another organization (`ext`), within the
/// organization (`org`), at all (`shared`), or waiting to be (`pending`), and how many teams it
/// is connected and shared with. Of its group DMs, whether the name has Slack's `mpdm-` form.
pub(crate) fn channel_share(diag: &Diag, channels: &[Value]) {
    let mut share: BTreeMap<[String; 6], usize> = BTreeMap::new();
    let mut names: BTreeMap<[String; 1], usize> = BTreeMap::new();
    for c in channels.iter().take(super::ENTRIES_MAX) {
        if c.get("is_mpim").and_then(Value::as_bool) == Some(true) {
            let form = match c.get("name").and_then(Value::as_str) {
                Some(n) if n.starts_with("mpdm-") => "mpdm",
                Some(_) => "other",
                None => "absent",
            };
            *names.entry([form.to_string()]).or_default() += 1;
            continue;
        }
        if c.get("is_archived").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let flags = ["is_ext_shared", "is_org_shared", "is_shared", "is_pending_ext_shared"].map(|f| flag(c, f));
        let values = [
            flags[0].to_string(),
            flags[1].to_string(),
            flags[2].to_string(),
            flags[3].to_string(),
            size(c, "connected_team_ids"),
            size(c, "shared_team_ids"),
        ];
        *share.entry(values).or_default() += 1;
    }
    let share_names = ["ext", "org", "shared", "pending", "connected", "shared_teams"];
    tally_lines(diag, "channel-share", share_names, share);
    tally_lines(diag, "mpim-name", ["form"], names);
}

#[cfg(test)]
mod tests;
