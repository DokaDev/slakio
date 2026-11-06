//! Slack's own links, read so slakio can open them itself instead of the browser: a message's
//! link (`https://<name>.slack.com/archives/<conversation>/p<16 digits>`, a reply's with
//! `?thread_ts=<ts>&cid=<conversation>`), a conversation's (`…/archives/<conversation>`), the
//! same on an Enterprise Grid host (`<name>.enterprise.slack.com`), and the web client's
//! (`https://app.slack.com/client/<team>/<conversation>`).
//!
//! Only what reads exactly so is one: `https` only, the address passing [`opener::check`] first
//! (no user, no port, a plain host), the host one of Slack's by its whole last two labels (so
//! `slack.com.evil.test` or `evilslack.com` is not), ids of Slack's shape, a time of exactly 16
//! digits. Anything else is a web link like any other.

use crate::opener;
use slakio_core::model::Ts;

/// Where a Slack link says its workspace is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// A workspace's own host (`acme.slack.com`), lower case.
    Host(String),
    /// An Enterprise Grid organization's host (`acme.enterprise.slack.com`), lower case: any of
    /// its workspaces may hold the conversation.
    Enterprise(String),
    /// The web client's address names the workspace by its id (`T…`).
    Team(String),
}

/// A Slack link slakio can open itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlackLink {
    pub place: Place,
    /// The conversation's id (`C…`, `G…`, `D…`).
    pub conversation: String,
    /// The message it points at; `None` for a link to the conversation.
    pub message: Option<Ts>,
    /// The thread the message is a reply in (`thread_ts`), when it is one: never the message
    /// itself.
    pub thread: Option<Ts>,
}

/// Hosts under `slack.com` that are not a workspace's.
const NOT_WORKSPACES: [&str; 10] =
    ["api", "app", "edgeapi", "files", "slack-files", "status", "wss-primary", "www", "enterprise", "downloads"];

/// The Slack link `url` is, if it is one slakio can open itself.
pub fn parse(url: &str) -> Option<SlackLink> {
    let checked = opener::check(url)?;
    let rest = checked.url.strip_prefix("https://")?;
    let (authority, tail) = rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len()));
    // No port: the checked host is the whole authority.
    if authority != checked.host {
        return None;
    }
    let tail = tail.split_once('#').map_or(tail, |(before, _)| before);
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
    let path = path.strip_suffix('/').unwrap_or(path);
    let parts: Vec<&str> = path.split('/').skip(1).collect();
    let labels: Vec<&str> = checked.host.split('.').collect();
    let link = match labels[..] {
        ["app", "slack", "com"] => match parts[..] {
            ["client", team, conversation] if id(team, b"TE") && id(conversation, b"CGD") => SlackLink {
                place: Place::Team(team.to_string()),
                conversation: conversation.to_string(),
                message: None,
                thread: None,
            },
            _ => return None,
        },
        [name, "enterprise", "slack", "com"] if label(name) => {
            archives(Place::Enterprise(checked.host.clone()), &parts)?
        }
        [name, "slack", "com"] if label(name) && !NOT_WORKSPACES.contains(&name) => {
            archives(Place::Host(checked.host.clone()), &parts)?
        }
        _ => return None,
    };
    Some(with_thread(link, query))
}

/// `/archives/<conversation>` or `/archives/<conversation>/p<16 digits>` at `place`.
fn archives(place: Place, parts: &[&str]) -> Option<SlackLink> {
    let (conversation, message) = match parts {
        ["archives", c] => (*c, None),
        ["archives", c, p] => (*c, Some(p_ts(p)?)),
        _ => return None,
    };
    id(conversation, b"CGD").then(|| SlackLink { place, conversation: conversation.to_string(), message, thread: None })
}

/// The thread `query` names (`thread_ts`), for a message link: only when it reads as a time,
/// is not the message itself, and the conversation it names (`cid`), if any, is the link's.
fn with_thread(mut link: SlackLink, query: &str) -> SlackLink {
    let field = |key: &str| query.split('&').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='));
    let same = field("cid").is_none_or(|c| c == link.conversation);
    link.thread = field("thread_ts").and_then(dotted_ts).filter(|t| same && link.message.is_some_and(|m| m != *t));
    link
}

/// An id of Slack's shape: one of `first`, then 2 to 31 upper-case letters or digits.
fn id(s: &str, first: &[u8]) -> bool {
    let b = s.as_bytes();
    (3..=32).contains(&b.len())
        && first.contains(&b[0])
        && b[1..].iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// A host label of a workspace's name: letters, digits and `-`, not at either end.
fn label(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('-') && !s.ends_with('-')
}

/// `p` and exactly 16 digits (the time, its dot left out): a message link's time.
fn p_ts(s: &str) -> Option<Ts> {
    let digits = s.strip_prefix('p')?;
    if digits.len() != 16 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().map(Ts)
}

/// Slack's spelling of a time: 1 to 10 digits, a dot and exactly 6 digits.
fn dotted_ts(s: &str) -> Option<Ts> {
    let (secs, micros) = s.split_once('.')?;
    let digits = |d: &str| d.bytes().all(|b| b.is_ascii_digit());
    if !(1..=10).contains(&secs.len()) || micros.len() != 6 || !digits(secs) || !digits(micros) {
        return None;
    }
    Some(Ts(secs.parse::<u64>().ok()?.checked_mul(1_000_000)?.checked_add(micros.parse().ok()?)?))
}

#[cfg(test)]
mod tests;
