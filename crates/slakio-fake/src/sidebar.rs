//! The sidebar's sections (`users.channelSections.list`) as Slack answers them: the world's
//! sections of the workspace in its order, between sections of types a client does not show
//! (`temporary_channels` first, `agents` last), linked by `next_channel_section_id`. A section
//! the user made lists its conversations; Slack's own sections list none. The whole answer is
//! one page with a top-level `cursor` that is always there and names no next page; the older
//! spelling ([`paged`]) splits it over two pages named by `response_metadata.next_cursor`.

use crate::Logged;
use serde_json::{Value, json};
use slakio_core::model::SectionKind;
use slakio_world::World;

/// When every section was last changed.
const UPDATED: u64 = 1_767_603_600;

/// The cursor of the paged spelling's second page.
pub const PAGE_2: &str = "fake-page-2";

/// The sections of workspace `team`, in order, as Slack lists them.
fn sections(world: &World, team: usize) -> Option<Vec<Value>> {
    let s = world.snapshot();
    let ws = s.workspaces.get(team)?;
    let mut out =
        vec![json!({"channel_section_id": "LFAKETMP", "name": "", "type": "temporary_channels", "emoji": ""})];
    for (n, section) in s.sections.iter().filter(|x| x.workspace == ws.id).enumerate() {
        let (kind, name) = match section.kind {
            SectionKind::Favorites => ("stars", String::new()),
            SectionKind::Custom => ("standard", section.name.unsanitized().to_string()),
            SectionKind::Channels => ("channels", section.name.unsanitized().to_string()),
            SectionKind::DirectMessages => ("direct_messages", section.name.unsanitized().to_string()),
        };
        let ids: Vec<&str> = if section.kind == SectionKind::Custom {
            s.conversations.iter().filter(|c| c.section == section.id).map(|c| c.id.as_str()).collect()
        } else {
            Vec::new()
        };
        let id = format!("LFAKE{n}");
        let page = json!({"channel_ids": ids, "count": ids.len(), "cursor": format!("fake-ids-{n}"), "id": id});
        out.push(json!({"channel_section_id": id, "name": name, "type": kind, "emoji": "",
            "channel_ids_page": page}));
    }
    out.push(json!({"channel_section_id": "LFAKEAGENTS", "name": "Agents", "type": "agents", "emoji": ""}));
    let ids: Vec<Value> = out.iter().map(|s| s["channel_section_id"].clone()).collect();
    for (n, s) in out.iter_mut().enumerate() {
        s["next_channel_section_id"] = ids.get(n + 1).cloned().unwrap_or(Value::Null);
        s["last_updated"] = json!(UPDATED);
        s["is_redacted"] = json!(false);
        if s.get("channel_ids_page").is_none() {
            s["channel_ids_page"] = json!({"channel_ids": [], "count": 0, "cursor": ""});
        }
    }
    Some(out)
}

/// The answer as Slack gives it now: one page, a top-level `cursor` always there.
pub(crate) fn answer(world: &World, team: usize) -> Value {
    let Some(list) = sections(world, team) else { return json!({"ok": false, "error": "invalid_auth"}) };
    json!({"ok": true, "channel_sections": list.clone(), "cursor": "ZmFrZS10b3AtY3Vyc29y", "count": list.len(),
        "entities": {}, "last_updated": UPDATED})
}

/// The older spelling: two pages, the first naming the second by `response_metadata.next_cursor`.
pub(crate) fn paged(world: &World, team: usize, request: &Logged) -> Value {
    let Some(mut list) = sections(world, team) else { return json!({"ok": false, "error": "invalid_auth"}) };
    let count = list.len();
    let rest = list.split_off(count / 2);
    if crate::shapes::field(request, "cursor").as_deref() == Some(PAGE_2) {
        return json!({"ok": true, "channel_sections": rest, "count": count, "last_updated": UPDATED,
            "response_metadata": {"next_cursor": ""}});
    }
    json!({"ok": true, "channel_sections": list, "count": count, "last_updated": UPDATED,
        "response_metadata": {"next_cursor": PAGE_2}})
}
