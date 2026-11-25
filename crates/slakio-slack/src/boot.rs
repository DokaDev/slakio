//! A workspace's start: `client.userBoot` (the workspace, its channels and DMs, the user's
//! preferences and do-not-disturb times; it cannot do without), then what it can do without —
//! the unread counts (`client.counts`), the DMs' people by name (edge `users/info`) and whether
//! they are in do not disturb (`dnd.teamInfo`) — each of which, when it fails, is unknown and
//! never zero or "not in do not disturb". The user's own profile photo comes from the start
//! bundle's profile, else from the same edge lookup (the user asked for along with the DMs'
//! people), else from what a page already told of them.
//!
//! Counts are Slack's: a channel is unread when Slack says it has unreads and its mentions are
//! Slack's mention count; a DM's (and a group DM's) count is Slack's too, every message there
//! being for the user. A conversation the user muted keeps its counts here: the UI shows a muted
//! channel's mentions only, a muted DM's nothing. Slack Connect: a channel shared with another organization (`is_ext_shared`; one shared
//! within the organization is not), and a DM or group DM Slack marks so or with a person of
//! another team. Group DMs show when open in the sidebar (`is_open`); their names never hold the
//! user's own handle.

use crate::client::Client;
use crate::error::SlackError;
use crate::governor::Priority;
use crate::method::Method;
use crate::people::People;
use crate::request::Call;
use crate::sidebar::Facts;
use crate::wire;
use slakio_core::backend::{Event, Snapshot};
use slakio_core::diag::{Kind, Stage};
use slakio_core::model::{
    Conversation, ConversationId, ConversationKind, Org, Presence, Ts, User, UserId, Workspace, WorkspaceColor,
    WorkspaceId,
};
use slakio_core::sanitize::Remote;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, PoisonError};

/// The user's own id once the workspace has booted (whose reactions are "mine").
pub(crate) type Me = std::sync::Arc<Mutex<Option<UserId>>>;

/// How many people one `dnd.teamInfo` asks about.
pub const DND_BATCH: usize = 80;

/// The answer to a boot of `client`'s workspace.
pub(crate) async fn boot(client: &Client, me: &Me, people: &People) -> Result<Event, SlackError> {
    let boot: wire::UserBoot = client.call(&crate::login::user_boot(), Priority::User).await?;
    let (Some(team), Some(self_)) = (boot.team.as_ref(), boot.me.as_ref()) else {
        return Err(SlackError::Network { detail: "the start answer has no team or user".to_string() });
    };
    client.set_team(&team.id);
    people.set_team(&team.id);
    *me.lock().unwrap_or_else(PoisonError::into_inner) = Some(UserId::new(&self_.id));
    let counts = counts(client).await;
    // The sidebar's sections: unknown when they cannot be read (the two default ones then).
    let sidebar = crate::sidebar::Sidebar::new(&team.id, crate::sections::fetch(client).await, client.diag());
    // The DMs' people, by name (in batches); those not found show their id.
    let peers: Vec<String> = boot.ims.iter().flatten().filter_map(|im| im.user.clone()).collect();
    // The user too, when the start bundle has no photo of them; and the people of the group DMs
    // shown.
    let mut asked = peers.clone();
    let open: HashSet<&str> = boot.is_open.iter().flatten().map(String::as_str).collect();
    let grouped =
        boot.channels.iter().flatten().filter(|c| shown_group(c, &open)).flat_map(|c| c.members.iter().flatten());
    for m in grouped.filter(|m| **m != self_.id) {
        if !asked.contains(m) {
            asked.push(m.clone());
        }
    }
    let bundled = self_.profile.as_ref().and_then(wire::Profile::avatar).is_some();
    crate::shape::own_profile(client.diag(), self_);
    let before = people.standing(&self_.id);
    if !bundled {
        // A page may have named them already without a photo: asked all the same.
        people.forget_unpictured(&self_.id);
        asked.push(self_.id.clone());
        let standing = people.standing(&self_.id);
        let lookup = [("before", before), ("now", standing)];
        client.diag().line(Kind::SelfPhoto, Stage::Asked, if standing == "unknown" { "yes" } else { "no" }, &lookup);
    }
    let watched = (!bundled).then_some(self_.id.as_str());
    let (mut known, answer) = people.lookup(client, &asked, Priority::Background, watched).await;
    if client.diag().on() {
        if !bundled {
            crate::shape::own_lookup(client.diag(), people.standing(&self_.id), answer);
        }
        let photo = own_photo(self_, &known);
        let source = match (bundled, photo.is_some(), before) {
            (true, _, _) => "profile",
            (false, true, "pictured") => "page",
            (false, true, _) => "lookup",
            (false, false, _) => "none",
        };
        crate::shape::own_source(client.diag(), source, photo.as_deref());
    }
    let now = now_micros() / 1_000_000;
    let dnd = dnd(client, &peers, now).await;
    for u in &mut known {
        if let Some(&on) = dnd.get(u.id.as_str()) {
            u.presence = if on { Presence::Dnd } else { Presence::Unknown };
        }
    }
    Ok(Event::Booted(Box::new(snapshot(boot, counts, known, now, sidebar))))
}

/// The unread counts (`client.counts`, in the background share); `None` when they could not be
/// read. An answer without one of its lists cannot be read: unknown, not zero.
pub(crate) async fn counts(client: &Client) -> Option<wire::Counts> {
    client
        .call::<wire::Counts>(&Call::form(Method::ClientCounts, &[]), Priority::Background)
        .await
        .ok()
        .filter(|c| c.channels.is_some() && c.mpims.is_some() && c.ims.is_some())
}

/// Whether each of `ids` is in do not disturb at `now`, as far as Slack says (in batches of
/// [`DND_BATCH`]; a batch that fails tells nothing).
async fn dnd(client: &Client, ids: &[String], now: u64) -> HashMap<String, bool> {
    let mut wanted: Vec<&String> = ids.iter().collect();
    wanted.sort_unstable();
    wanted.dedup();
    let mut out = HashMap::new();
    for batch in wanted.chunks(DND_BATCH) {
        let users = batch.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",");
        let call = Call::form(Method::DndTeamInfo, &[("users", users.as_str())]);
        let Ok(team) = client.call::<wire::DndTeam>(&call, Priority::Background).await else { continue };
        for (id, v) in team.users.unwrap_or_default() {
            if let Some(on) = serde_json::from_value::<wire::Dnd>(v).ok().and_then(|d| d.active(now)) {
                out.insert(id, on);
            }
        }
    }
    out
}

/// The unread counts of each conversation Slack listed: (unread, mentions).
pub(crate) fn unread_counts(counts: wire::Counts) -> HashMap<String, (u32, u32)> {
    let mut out = HashMap::new();
    for n in [counts.channels, counts.mpims, counts.ims].into_iter().flatten().flatten() {
        let mentions = n.mention_count.unwrap_or(0);
        // Slack has no count of a channel's unread messages: one stands for "some".
        let unread = if n.has_unreads == Some(true) { mentions.max(1) } else { mentions };
        out.insert(n.id, (unread, mentions.min(unread)));
    }
    out
}

/// The snapshot of `boot` (with a team and a user), its counts `counts` (`None`: unknown), the
/// people `known`, at `now` (seconds), its conversations placed in the sections of `sidebar`.
fn snapshot(
    boot: wire::UserBoot,
    counts: Option<wire::Counts>,
    known: Vec<User>,
    now: u64,
    sidebar: crate::sidebar::Sidebar,
) -> Snapshot {
    let (Some(team), Some(me)) = (boot.team, boot.me) else { return Snapshot::default() };
    let ws = WorkspaceId::new(&team.id);
    let counts_failed = counts.is_none();
    let unread = counts.map(unread_counts).unwrap_or_default();
    let muted: HashSet<String> = boot.prefs.as_ref().map(wire::Prefs::muted).unwrap_or_default();
    let starred: HashSet<String> = boot.starred.unwrap_or_default().into_iter().collect();
    let open_ids = boot.is_open.unwrap_or_default();
    let open: HashSet<&str> = open_ids.iter().map(String::as_str).collect();
    let conversation = |id: String, name: String, kind: ConversationKind, facts: Facts| {
        let (unread, mentions) = unread.get(&id).copied().unwrap_or_default();
        let section = sidebar.place(&id, facts);
        let external = facts.external;
        Conversation {
            muted: muted.contains(&id),
            id: ConversationId::new(id),
            workspace: ws.clone(),
            kind,
            name: Remote::new(name),
            section,
            external,
            unread,
            mentions,
        }
    };
    let channels = boot.channels.unwrap_or_default();
    let listed = listed(channels, boot.ims.unwrap_or_default(), (&starred, &open), &known, &me);
    let conversations: Vec<Conversation> =
        listed.into_iter().map(|(id, name, kind, facts)| conversation(id, name, kind, facts)).collect();
    let me_name = me.name.clone().unwrap_or_else(|| me.id.clone());
    let avatar = own_photo(&me, &known);
    // Slack said the user has no photo of their own: in the start bundle, or the lookup.
    let not_custom = me.profile.as_ref().is_some_and(wire::Profile::not_custom)
        || known.iter().any(|u| u.id.as_str() == me.id && u.no_photo);
    let presence = match boot.dnd.as_ref().and_then(|d| d.active(now)) {
        Some(true) => Presence::Dnd,
        _ => Presence::Unknown,
    };
    Snapshot {
        workspaces: vec![Workspace {
            id: ws.clone(),
            name: Remote::new(team.name.clone().unwrap_or_else(|| team.id.clone())),
            color: WorkspaceColor(0),
            me: UserId::new(&me.id),
        }],
        users: std::iter::once(User {
            id: UserId::new(&me.id),
            workspace: ws,
            name: Remote::new(me_name.clone()),
            display_name: Remote::new(me_name),
            org: Org::Own,
            bot: false,
            presence,
            no_photo: avatar.is_none() && not_custom,
            avatar,
        })
        .chain(known.into_iter().filter(|u| u.id.as_str() != me.id))
        .collect(),
        sections: sidebar.sections,
        conversations,
        // Saved items are not read in this version: unknown, not none.
        saved: None,
        taken: Ts(now_micros()),
        counts_failed,
    }
}

/// The conversations of the start bundle's `channels` (group DMs among them) and `ims`, the
/// sidebar shows: each one's id, name, kind and what places it (`starred`, `open`: the start
/// bundle's lists; `known`: the people named; `me`: the user).
fn listed(
    channels: Vec<wire::Channel>,
    ims: Vec<wire::Im>,
    (starred, open): (&HashSet<String>, &HashSet<&str>),
    known: &[User],
    me: &wire::Me,
) -> Vec<(String, String, ConversationKind, Facts)> {
    let peer_of = |id: &str| known.iter().find(|u| u.id.as_str() == id);
    let outside = |id: &str| peer_of(id).is_some_and(|u| matches!(u.org, Org::External(_)));
    // The user's own handle: the start bundle's, else the one looked up.
    let own = me.name.as_deref().or_else(|| peer_of(&me.id).map(|u| u.name.unsanitized()));
    let mut out = Vec::new();
    for c in channels.into_iter().filter(|c| c.is_archived != Some(true)) {
        let star = c.is_starred == Some(true) || starred.contains(&c.id);
        if c.is_mpim != Some(true) {
            let kind = ConversationKind::Channel { private: c.is_private == Some(true) };
            let facts = Facts { starred: star, external: c.is_ext_shared == Some(true), ..Facts::default() };
            out.push((c.id.clone(), c.name.unwrap_or(c.id), kind, facts));
            continue;
        }
        // A group DM: shown when open in the sidebar.
        if !shown_group(&c, open) {
            continue;
        }
        // Its people: its members but the user; without members, the people known by the
        // handles of its name (the user's left out).
        let users: Vec<UserId> = match &c.members {
            Some(members) => members.iter().filter(|m| **m != me.id).map(UserId::new).collect(),
            None => handles(c.name.as_deref(), own)
                .iter()
                .filter_map(|h| known.iter().find(|k| k.name.unsanitized() == *h && k.id.as_str() != me.id))
                .map(|k| k.id.clone())
                .collect(),
        };
        // Named by its people only when Slack listed them all.
        let named: &[UserId] = if c.members.is_some() { &users } else { &[] };
        let name = group_name(named, known, c.name.as_deref(), own).unwrap_or_else(|| c.id.clone());
        let external = c.is_ext_shared == Some(true) || users.iter().any(|u| outside(u.as_str()));
        let facts = Facts { dm: true, starred: star, external, app: false };
        out.push((c.id, name, ConversationKind::GroupDm { users }, facts));
    }
    out.extend(ims.into_iter().filter_map(|im| {
        let user = im.user?;
        let peer = peer_of(&user);
        let name = peer.map_or_else(|| user.clone(), |u| u.display_name.unsanitized().to_string());
        let shared = im.is_ext_shared == Some(true) || im.connected_team_ids.as_ref().is_some_and(|t| !t.is_empty());
        let facts = Facts {
            dm: true,
            starred: im.is_starred == Some(true) || starred.contains(&im.id),
            external: shared || outside(&user),
            app: peer.is_some_and(|u| u.bot),
        };
        Some((im.id, name, ConversationKind::Dm { user: UserId::new(user) }, facts))
    }));
    out
}

/// A group DM the sidebar shows: not archived, and open (`open`: the start bundle's `is_open`).
fn shown_group(c: &wire::Channel, open: &HashSet<&str>) -> bool {
    c.is_mpim == Some(true) && c.is_archived != Some(true) && open.contains(c.id.as_str())
}

/// A group DM's name: its people's names (`users`, the user left out) when all are known, else
/// the handles in Slack's `mpdm-one--two--three-1` (`own`: the user's, left out).
fn group_name(users: &[UserId], known: &[User], handle: Option<&str>, own: Option<&str>) -> Option<String> {
    let names: Option<Vec<&str>> =
        users.iter().map(|u| known.iter().find(|k| k.id == *u).map(|k| k.display_name.unsanitized())).collect();
    if let Some(names) = names.filter(|n| !n.is_empty()) {
        return Some(names.join(", "));
    }
    let handles = handles(handle, own);
    (!handles.is_empty()).then(|| handles.join(", "))
}

/// The handles in Slack's group DM name `mpdm-one--two--three-1`, the user's (`own`) left out;
/// none in a name of another shape.
fn handles<'a>(handle: Option<&'a str>, own: Option<&str>) -> Vec<&'a str> {
    let Some(inner) = handle.and_then(|h| h.strip_prefix("mpdm-")) else { return Vec::new() };
    let inner = inner
        .rsplit_once('-')
        .filter(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        .map_or(inner, |(h, _)| h);
    inner.split("--").filter(|h| !h.is_empty() && Some(*h) != own).collect()
}

/// The user's own photo: the start bundle's, else the one looked up or learned of them
/// (`known`).
fn own_photo(me: &wire::Me, known: &[User]) -> Option<String> {
    me.profile
        .as_ref()
        .and_then(wire::Profile::avatar)
        .or_else(|| known.iter().find(|u| u.id.as_str() == me.id).and_then(|u| u.avatar.clone()))
}

pub(crate) fn now_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests;
