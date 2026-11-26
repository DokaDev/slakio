//! Probes of what Slack says of people's status: whether the start bundle's own profile has the
//! status fields, and how many profiles of a people lookup carry a status. Flags and counts only:
//! never an emoji, a text, a time, a name or an id. They tell, from the owner's own run, whether
//! the status the sidebar shows reads the right fields.

use crate::wire;
use slakio_core::diag::{Diag, Kind, Stage};

/// `y` / `n`.
fn yn(b: bool) -> &'static str {
    if b { "y" } else { "n" }
}

/// The start bundle's own profile: whether it has a readable `status_emoji` (`present`), whether
/// that is set (`set`: `-` when not present), and whether a set one has an end (`expires`: `-`
/// when not set).
pub(crate) fn self_status(diag: &Diag, boot: &wire::UserBoot) {
    let profile = boot.me.as_ref().and_then(|m| m.profile.as_ref());
    let said = profile.and_then(wire::Profile::status);
    let set = said.as_ref().map_or("-", |s| yn(s.is_some()));
    let expires = said.flatten().map_or("-", |s| yn(s.expires.is_some()));
    let pairs = [("present", yn(set != "-")), ("set", set), ("expires", expires)];
    diag.line(Kind::BootShape, Stage::Field, "self-status", &pairs);
}

/// A people lookup's answer `persons`: how many carry a status (`n`), how many profiles say they
/// have none (`empty`, an empty `status_emoji`), how many profiles do not say (`absent`: no
/// readable `status_emoji`), and how many have no profile at all (`no_profile`).
pub(crate) fn statuses(diag: &Diag, persons: &[wire::Person]) {
    let (mut set, mut empty, mut absent, mut no_profile) = (0usize, 0usize, 0usize, 0usize);
    for p in persons.iter().take(super::ENTRIES_MAX) {
        match p.profile.as_ref().map(wire::Profile::status) {
            None => no_profile += 1,
            Some(None) => absent += 1,
            Some(Some(None)) => empty += 1,
            Some(Some(Some(_))) => set += 1,
        }
    }
    let counts = [set, empty, absent, no_profile].map(|c| c.to_string());
    let pairs = [("n", &*counts[0]), ("empty", &*counts[1]), ("absent", &*counts[2]), ("no_profile", &*counts[3])];
    diag.line(Kind::People, Stage::Field, "status", &pairs);
}
