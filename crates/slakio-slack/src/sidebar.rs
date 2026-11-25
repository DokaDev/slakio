//! The sidebar's sections as Slack lists them for the user, in the order of its
//! `next_channel_section_id` chain, mapped onto the core's kinds: `stars` (Starred),
//! `standard` (a section the user made, with the conversations of its first page of ids),
//! `slack_connect` (External connections), `channels`, `direct_messages` and `recent_apps`
//! (Apps). The other types (`temporary_channels`, `salesforce_records`, `agents`, any unknown)
//! are not shown. A section without a name of Slack's gets its English one; Channels and Direct
//! messages are always there (appended when Slack listed none), so every conversation has a
//! place; with no list at all the sidebar is those two alone.
//!
//! A conversation goes to the first of: a section the user made that lists it, Starred (when
//! starred), External connections (when shared with another organization), Apps (a DM with an
//! app), then Channels or Direct messages; each of Starred, External connections and Apps only
//! when Slack lists it for the user.
//!
//! A section the user made lists only the conversations of its first page of ids: no allowed
//! method reads the others, which stay in their default section (the diagnostics tell the gap).

use serde::Deserialize;
use slakio_core::diag::{Diag, Kind, Stage};
use slakio_core::model::{Section, SectionId, SectionKind, WorkspaceId};
use slakio_core::sanitize::Remote;
use std::collections::{HashMap, HashSet};

/// One section of `users.channelSections.list`.
#[derive(Debug, Deserialize)]
pub struct ChannelSection {
    pub channel_section_id: String,
    #[serde(rename = "type", default, deserialize_with = "crate::wire::loose")]
    pub kind: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::loose")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::loose")]
    pub next_channel_section_id: Option<String>,
    #[serde(default, deserialize_with = "crate::wire::loose")]
    pub channel_ids_page: Option<IdsPage>,
}

/// A section's first page of conversation ids.
#[derive(Debug, Default, Deserialize)]
pub struct IdsPage {
    #[serde(default, deserialize_with = "crate::wire::lenient")]
    pub channel_ids: Option<Vec<String>>,
    #[serde(default, deserialize_with = "crate::wire::loose")]
    pub count: Option<u64>,
}

/// `list` in the order of its chain: from the section no other names as next (the first
/// listed when every one is named), each one once; a cycle ends the chain, and the sections it
/// never reached follow in the order listed. Of sections with the same id, the first listed.
pub(crate) fn order(list: Vec<ChannelSection>) -> Vec<ChannelSection> {
    let mut ids = HashSet::new();
    let list: Vec<ChannelSection> = list.into_iter().filter(|s| ids.insert(s.channel_section_id.clone())).collect();
    let at: HashMap<&str, usize> = list.iter().enumerate().map(|(i, s)| (s.channel_section_id.as_str(), i)).collect();
    let named: HashSet<&str> = list
        .iter()
        .filter_map(|s| s.next_channel_section_id.as_deref().filter(|n| *n != s.channel_section_id))
        .collect();
    let head = list.iter().position(|s| !named.contains(s.channel_section_id.as_str())).unwrap_or(0);
    let (mut chain, mut taken) = (Vec::with_capacity(list.len()), vec![false; list.len()]);
    let mut next = (!list.is_empty()).then_some(head);
    while let Some(i) = next.filter(|i| !taken[*i]) {
        taken[i] = true;
        chain.push(i);
        next = list[i].next_channel_section_id.as_deref().and_then(|n| at.get(n).copied());
    }
    chain.extend((0..list.len()).filter(|i| !taken[*i]));
    let mut slots: Vec<Option<ChannelSection>> = list.into_iter().map(Some).collect();
    chain.into_iter().filter_map(|i| slots[i].take()).collect()
}

/// The core's kind of a Slack section type, and its English name; `None`: not shown.
fn kind(t: &str) -> Option<(SectionKind, &'static str)> {
    Some(match t {
        "stars" => (SectionKind::Favorites, "Starred"),
        "standard" => (SectionKind::Custom, "Section"),
        "slack_connect" => (SectionKind::External, "External connections"),
        "channels" => (SectionKind::Channels, "Channels"),
        "direct_messages" => (SectionKind::DirectMessages, "Direct messages"),
        "recent_apps" => (SectionKind::Apps, "Apps"),
        _ => return None,
    })
}

/// The sidebar of a workspace: its sections in order, and where a conversation goes.
#[derive(Debug)]
pub(crate) struct Sidebar {
    pub(crate) sections: Vec<Section>,
    /// The section the user made that lists a conversation (the first in order).
    custom: HashMap<String, SectionId>,
    starred: Option<SectionId>,
    external: Option<SectionId>,
    apps: Option<SectionId>,
    channels: SectionId,
    dms: SectionId,
}

/// What decides where a conversation goes, besides the sections the user made.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Facts {
    /// A DM or a group DM.
    pub(crate) dm: bool,
    pub(crate) starred: bool,
    /// Shared with another organization (Slack Connect).
    pub(crate) external: bool,
    /// A DM with an app.
    pub(crate) app: bool,
}

impl Sidebar {
    /// The sidebar of workspace `team` from Slack's `listed` sections (`None`: unknown, the two
    /// default sections alone); a section's ids beyond its first page are told to `diag`.
    pub(crate) fn new(team: &str, listed: Option<Vec<ChannelSection>>, diag: &Diag) -> Self {
        let ws = WorkspaceId::new(team);
        let section = |id: String, name: &str, kind| Section {
            id: SectionId::new(format!("{team}:{id}")),
            workspace: ws.clone(),
            name: Remote::new(name),
            kind,
        };
        let (mut sections, mut custom) = (Vec::new(), HashMap::new());
        for (n, s) in order(listed.unwrap_or_default()).into_iter().enumerate() {
            let Some((kind, english)) = s.kind.as_deref().and_then(kind) else { continue };
            let name = s.name.as_deref().map(str::trim).filter(|t| !t.is_empty()).unwrap_or(english);
            let id = match kind {
                SectionKind::Custom => s.channel_section_id.clone(),
                SectionKind::Favorites => "stars".to_string(),
                SectionKind::External => "external".to_string(),
                SectionKind::Channels => "channels".to_string(),
                SectionKind::DirectMessages => "dms".to_string(),
                SectionKind::Apps => "apps".to_string(),
            };
            // Slack's own sections once each.
            if kind != SectionKind::Custom && sections.iter().any(|x: &Section| x.kind == kind) {
                continue;
            }
            let made = section(id, name, kind);
            if kind == SectionKind::Custom {
                let page = s.channel_ids_page.unwrap_or_default();
                let ids = page.channel_ids.unwrap_or_default();
                if page.count.is_some_and(|c| c > ids.len() as u64) {
                    let (n, ids, count) = (n.to_string(), ids.len().to_string(), page.count.unwrap_or(0).to_string());
                    diag.line(
                        Kind::Sections,
                        Stage::Skipped,
                        "more-ids",
                        &[("n", &n), ("ids", &ids), ("count", &count)],
                    );
                }
                for id in ids {
                    custom.entry(id).or_insert_with(|| made.id.clone());
                }
            }
            sections.push(made);
        }
        for (kind, id, name) in
            [(SectionKind::Channels, "channels", "Channels"), (SectionKind::DirectMessages, "dms", "Direct messages")]
        {
            if !sections.iter().any(|s| s.kind == kind) {
                sections.push(section(id.to_string(), name, kind));
            }
        }
        // Both are there (appended above when Slack listed none).
        let of = |kind, id: &str| {
            sections
                .iter()
                .find(|s| s.kind == kind)
                .map_or_else(|| SectionId::new(format!("{team}:{id}")), |s| s.id.clone())
        };
        let (channels, dms) = (of(SectionKind::Channels, "channels"), of(SectionKind::DirectMessages, "dms"));
        let listed = |kind| sections.iter().find(|s| s.kind == kind).map(|s| s.id.clone());
        let (starred, external, apps) =
            (listed(SectionKind::Favorites), listed(SectionKind::External), listed(SectionKind::Apps));
        Self { sections, custom, starred, external, apps, channels, dms }
    }

    /// The section conversation `id` is listed in.
    pub(crate) fn place(&self, id: &str, f: Facts) -> SectionId {
        let pick = |on: bool, s: &Option<SectionId>| s.as_ref().filter(|_| on).cloned();
        self.custom
            .get(id)
            .cloned()
            .or_else(|| pick(f.starred, &self.starred))
            .or_else(|| pick(f.external, &self.external))
            .or_else(|| pick(f.app && f.dm, &self.apps))
            .unwrap_or_else(|| if f.dm { self.dms.clone() } else { self.channels.clone() })
    }
}

#[cfg(test)]
mod tests;
