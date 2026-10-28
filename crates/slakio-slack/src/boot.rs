//! A workspace's start: `client.userBoot` (the workspace, its channels and DMs, the user's
//! preferences and do-not-disturb times; it cannot do without), then what it can do without —
//! the unread counts (`client.counts`), the DMs' people by name (edge `users/info`) and whether
//! they are in do not disturb (`dnd.teamInfo`) — each of which, when it fails, is unknown and
//! never zero or "not in do not disturb".
//!
//! Counts are Slack's: a channel is unread when Slack says it has unreads and its mentions are
//! Slack's mention count; a DM's (and a group DM's) count is Slack's too, every message there
//! being for the user. A conversation the user muted keeps its counts here: the UI shows a muted
//! channel's mentions only, a muted DM's nothing. Slack Connect: a channel shared with another organization (`is_ext_shared`; one shared
//! within the organization is not), and a DM with a person of another team.

use crate::client::Client;
use crate::error::SlackError;
use crate::governor::Priority;
use crate::method::Method;
use crate::people::People;
use crate::request::Call;
use crate::wire;
use slakio_core::backend::{Event, Snapshot};
use slakio_core::model::{
    Conversation, ConversationId, ConversationKind, Org, Presence, Section, SectionId, SectionKind, Ts, User, UserId,
    Workspace, WorkspaceColor, WorkspaceId,
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
    // An answer without one of its lists cannot be read: unknown, not zero.
    let counts: Option<wire::Counts> = client
        .call::<wire::Counts>(&Call::form(Method::ClientCounts, &[]), Priority::Background)
        .await
        .ok()
        .filter(|c| c.channels.is_some() && c.mpims.is_some() && c.ims.is_some());
    // The DMs' people, by name (in batches); those not found show their id.
    let peers: Vec<String> = boot.ims.iter().flatten().filter_map(|im| im.user.clone()).collect();
    let mut known = people.names(client, &peers, Priority::Background).await;
    let now = now_micros() / 1_000_000;
    let dnd = dnd(client, &peers, now).await;
    for u in &mut known {
        if let Some(&on) = dnd.get(u.id.as_str()) {
            u.presence = if on { Presence::Dnd } else { Presence::Unknown };
        }
    }
    Ok(Event::Booted(Box::new(snapshot(boot, counts, known, now))))
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
fn unread_counts(counts: wire::Counts) -> HashMap<String, (u32, u32)> {
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
/// people `known`, at `now` (seconds).
fn snapshot(boot: wire::UserBoot, counts: Option<wire::Counts>, known: Vec<User>, now: u64) -> Snapshot {
    let (Some(team), Some(me)) = (boot.team, boot.me) else { return Snapshot::default() };
    let ws = WorkspaceId::new(&team.id);
    let counts_failed = counts.is_none();
    let unread = counts.map(unread_counts).unwrap_or_default();
    let muted: HashSet<String> = boot.prefs.as_ref().map(wire::Prefs::muted).unwrap_or_default();
    let section = |id: &str, name: &str, kind| Section {
        id: SectionId::new(format!("{}:{id}", team.id)),
        workspace: ws.clone(),
        name: Remote::new(name),
        kind,
    };
    let sections = vec![
        section("channels", "Channels", SectionKind::Channels),
        section("dms", "Direct messages", SectionKind::DirectMessages),
    ];
    let conversation = |id: String, name: String, kind: ConversationKind, external: bool, section: &Section| {
        let (unread, mentions) = unread.get(&id).copied().unwrap_or_default();
        Conversation {
            muted: muted.contains(&id),
            id: ConversationId::new(id),
            workspace: ws.clone(),
            kind,
            name: Remote::new(name),
            section: section.id.clone(),
            external,
            unread,
            mentions,
        }
    };
    let mut conversations: Vec<Conversation> = boot
        .channels
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.is_archived != Some(true) && c.is_mpim != Some(true))
        .map(|c| {
            let name = c.name.unwrap_or_else(|| c.id.clone());
            let kind = ConversationKind::Channel { private: c.is_private == Some(true) };
            conversation(c.id, name, kind, c.is_ext_shared == Some(true), &sections[0])
        })
        .collect();
    conversations.extend(boot.ims.unwrap_or_default().into_iter().filter_map(|im| {
        let user = im.user?;
        let peer = known.iter().find(|u| u.id.as_str() == user);
        let name = peer.map_or_else(|| user.clone(), |u| u.display_name.unsanitized().to_string());
        let external = peer.is_some_and(|u| matches!(u.org, Org::External(_)));
        Some(conversation(im.id, name, ConversationKind::Dm { user: UserId::new(user) }, external, &sections[1]))
    }));
    let me_name = me.name.clone().unwrap_or_else(|| me.id.clone());
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
        })
        .chain(known.into_iter().filter(|u| u.id.as_str() != me.id))
        .collect(),
        sections,
        conversations,
        // Saved items are not read in this version: unknown, not none.
        saved: None,
        taken: Ts(now_micros()),
        counts_failed,
    }
}

pub(crate) fn now_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests;
