//! A group DM's name: its people, two named and how many more (`Ann, Bob and 6 others`), in the
//! UI language. Only a group DM named by its people's names (as the boot names it) is named so;
//! one named otherwise (Slack's handles, people not known) keeps its name as it is.

use crate::app::model::Model;
use crate::text::{clip, width};
use slakio_core::i18n::{I18n, Msg};
use slakio_core::model::{Conversation, ConversationKind};

/// The names of `c`'s people (the user left out), when `c` is a group DM named by exactly those
/// names; else none.
pub fn people(model: &Model, c: &Conversation) -> Option<Vec<String>> {
    let ConversationKind::GroupDm { users } = &c.kind else { return None };
    let known: Option<Vec<_>> = users.iter().map(|id| model.user(&c.workspace, id)).collect();
    let known = known.filter(|k| !k.is_empty())?;
    let raw: Vec<&str> = known.iter().map(|u| u.display_name.unsanitized()).collect();
    if raw.iter().any(|n| n.trim().is_empty()) || c.name != raw.join(", ").as_str() {
        return None;
    }
    Some(known.iter().map(|u| u.display_name.line().into_string()).collect())
}

/// `names` in short: up to two joined by `, `; more, two and how many others.
pub fn short(names: &[String], i18n: &I18n) -> String {
    match names {
        [_, _, _, ..] => others(&names[..2].join(", "), names.len() - 2, i18n),
        _ => names.join(", "),
    }
}

/// `names` in `room` cells: [`short`] when it fits; else one name and how many others; else
/// that one name cut, the others said whole. Up to two names are cut as one text.
pub fn fit(names: &[String], i18n: &I18n, room: usize) -> String {
    let full = short(names, i18n);
    if width(&full) <= room {
        return full;
    }
    let [first, _, _, ..] = names else { return clip(&full, room) };
    let rest = names.len() - 1;
    let one = others(first, rest, i18n);
    if width(&one) <= room {
        return one;
    }
    let left = room.saturating_sub(width(&others("", rest, i18n)));
    if left == 0 {
        return clip(&one, room);
    }
    others(&clip(first, left), rest, i18n)
}

/// How `c` is named where it is named in short (titles, tabs, the status line): a group DM by
/// [`short`], anything else (and a group DM not named by its people) by its name.
pub fn name(model: &Model, i18n: &I18n, c: &Conversation) -> String {
    people(model, c).map_or_else(|| c.name.line().into_string(), |names| short(&names, i18n))
}

/// The badge of a group DM of `n` people (the user left out): `n` up to 9, else `9+`.
pub fn count(n: usize) -> String {
    if n > 9 { "9+".to_string() } else { n.to_string() }
}

fn others(names: &str, count: usize, i18n: &I18n) -> String {
    i18n.msg(&Msg::GroupOthers { names: names.to_string(), count: count as u64 }).to_string()
}

#[cfg(test)]
mod tests;
