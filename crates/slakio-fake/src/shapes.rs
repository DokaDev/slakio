//! The fake's answers, in the shapes of Slack's: built from the world's first workspace. Every
//! answer carries a field no client knows, and some entries leave optional fields out.

use crate::Logged;
use serde_json::{Value, json};
use slakio_core::model::{ConversationId, ConversationKind, Message, Org, Presence, Ts};
use slakio_world::World;

/// The answer to `request`.
pub(crate) fn answer(world: &World, team: usize, request: &Logged) -> Value {
    let mut out = match request.method() {
        "client.userBoot" => user_boot(world, team),
        "client.counts" => counts(world, team),
        "conversations.view" => view(world, request),
        "conversations.history" => history(world, request),
        "conversations.replies" => replies(world, request),
        "dnd.teamInfo" => dnd_team(world, request),
        "users/info" => edge_users(world, request),
        _ => json!({"ok": false, "error": "unknown_method"}),
    };
    if let Some(o) = out.as_object_mut() {
        o.insert("fake_unknown_field".into(), json!({"nested": [1, "two", null]}));
    }
    out
}

/// The value of form field `key` of `request`, decoded.
fn field(request: &Logged, key: &str) -> Option<String> {
    request.fields().into_iter().find(|(k, _)| *k == key).map(|(_, v)| decode(v))
}

fn decode(v: &str) -> String {
    let bytes = v.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(b) => {
                        out.push(b);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The workspace this fake serves: the world's `team`-th.
fn served(world: &World, team: usize) -> Option<&slakio_core::model::Workspace> {
    world.snapshot().workspaces.get(team)
}

fn user_boot(world: &World, team: usize) -> Value {
    let Some(ws) = served(world, team) else { return json!({"ok": false, "error": "invalid_auth"}) };
    let s = world.snapshot();
    let me = s.users.iter().find(|u| u.id == ws.me);
    let mut channels = Vec::new();
    let mut ims = Vec::new();
    for (n, c) in s.conversations.iter().filter(|c| c.workspace == ws.id).enumerate() {
        match &c.kind {
            ConversationKind::Channel { private } => {
                let mut ch = json!({"id": c.id.as_str(), "name": c.name.unsanitized(), "is_channel": true, "updated": 1_767_603_600_000_u64});
                // Every third channel leaves its optional flags out.
                if n % 3 != 0 {
                    ch["is_private"] = json!(private);
                    ch["is_ext_shared"] = json!(c.external);
                    ch["is_shared"] = json!(c.external);
                    ch["is_org_shared"] = json!(false);
                    ch["is_archived"] = json!(false);
                }
                channels.push(ch);
            }
            ConversationKind::Dm { user } => {
                ims.push(json!({"id": c.id.as_str(), "user": user.as_str(), "is_im": true, "is_open": true}))
            }
            ConversationKind::GroupDm { .. } => {
                channels.push(json!({"id": c.id.as_str(), "name": c.name.unsanitized(), "is_mpim": true}));
            }
        }
    }
    // Muted conversations as Slack spells them now (notification prefs as JSON in a string),
    // and every other one in the older comma-separated spelling too.
    let muted: Vec<&str> =
        s.conversations.iter().filter(|c| c.workspace == ws.id && c.muted).map(|c| c.id.as_str()).collect();
    let mut prefs_channels = serde_json::Map::new();
    for (n, id) in muted.iter().enumerate() {
        if n % 2 == 0 {
            prefs_channels.insert((*id).to_string(), json!({"muted": true, "desktop": "everything"}));
        }
    }
    let legacy: Vec<&str> = muted.iter().copied().skip(1).step_by(2).collect();
    let all_prefs = json!({"channels": prefs_channels, "global": {"global_desktop": "mentions"}}).to_string();
    let dnd = match me.map(|u| u.presence) {
        Some(Presence::Dnd) => dnd_on(),
        _ => json!({"dnd_enabled": false, "next_dnd_start_ts": 1, "next_dnd_end_ts": 1, "snooze_enabled": false}),
    };
    json!({
        "ok": true,
        "self": {"id": ws.me.as_str(), "name": me.map(|u| u.name.unsanitized()), "team_id": ws.id.as_str()},
        "team": {"id": ws.id.as_str(), "name": ws.name.unsanitized(), "domain": "fake"},
        "channels": channels,
        "ims": ims,
        "is_open": [],
        "prefs": {"all_notifications_prefs": all_prefs, "muted_channels": legacy.join(","), "theme": "light"},
        "dnd": dnd,
    })
}

fn counts(world: &World, team: usize) -> Value {
    let Some(ws) = served(world, team) else { return json!({"ok": false, "error": "invalid_auth"}) };
    let (mut channels, mut ims, mut mpims) = (Vec::new(), Vec::new(), Vec::new());
    for c in world.snapshot().conversations.iter().filter(|c| c.workspace == ws.id) {
        // A DM's mention count is its unread messages, as Slack counts them.
        let mentions = if c.is_dm() { c.unread } else { c.mentions };
        let entry = json!({"id": c.id.as_str(), "has_unreads": c.unread > 0, "mention_count": mentions, "last_read": "0000000000.000000"});
        match c.kind {
            ConversationKind::Channel { .. } => channels.push(entry),
            ConversationKind::Dm { .. } => ims.push(entry),
            ConversationKind::GroupDm { .. } => mpims.push(entry),
        }
    }
    json!({"ok": true, "channels": channels, "ims": ims, "mpims": mpims, "threads": {"has_unreads": false, "mention_count": 0}})
}

fn limit(request: &Logged, key: &str) -> usize {
    field(request, key).and_then(|v| v.parse().ok()).unwrap_or(28).min(1000)
}

fn ts_of(v: &str) -> Option<Ts> {
    let (s, m) = v.split_once('.')?;
    Some(Ts(s.parse::<u64>().ok()? * 1_000_000 + m.parse::<u64>().ok()?))
}

/// Messages newest first, as Slack lists them.
fn page(messages: Vec<Message>, complete: bool) -> Value {
    let list: Vec<Value> = messages.iter().rev().enumerate().map(|(n, m)| message(m, n)).collect();
    json!({"messages": list, "has_more": !complete})
}

fn message(m: &Message, n: usize) -> Value {
    let mut v =
        json!({"type": "message", "ts": m.ts.to_string(), "user": m.user.as_str(), "text": m.text.unsanitized()});
    if let Some(t) = m.thread {
        v["reply_count"] = json!(t.replies);
        v["latest_reply"] = json!(t.last_reply.to_string());
        v["thread_ts"] = json!(m.ts.to_string());
    }
    if !m.reactions.is_empty() {
        let r: Vec<Value> =
            m.reactions.iter().map(|r| json!({"name": r.name.unsanitized(), "count": r.count, "users": []})).collect();
        v["reactions"] = json!(r);
    }
    if m.edited {
        v["edited"] = json!({"user": m.user.as_str(), "ts": m.ts.to_string()});
    }
    // Some messages carry blocks the client does not read.
    if n % 4 == 1 {
        v["blocks"] = json!([{"type": "rich_text", "elements": []}]);
    }
    v
}

fn channel(request: &Logged) -> Option<ConversationId> {
    field(request, "channel").map(ConversationId::new)
}

fn view(world: &World, request: &Logged) -> Value {
    let Some(c) = channel(request).filter(|c| world.message_count(c) > 0) else {
        return json!({"ok": false, "error": "channel_not_found"});
    };
    let (messages, complete) = world.history(&c, None, limit(request, "count"));
    // The authors, as Slack sends them with a view: here only every other one, so the client's
    // lookup of the rest is exercised too.
    let mut authors: Vec<&str> = messages.iter().map(|m| m.user.as_str()).collect();
    authors.sort_unstable();
    authors.dedup();
    let users: Vec<Value> = authors.iter().step_by(2).filter_map(|id| person(world, id)).collect();
    json!({"ok": true, "history": page(messages, complete), "users": users, "channels": [], "emojis": {}})
}

fn history(world: &World, request: &Logged) -> Value {
    let Some(c) = channel(request).filter(|c| world.message_count(c) > 0) else {
        return json!({"ok": false, "error": "channel_not_found"});
    };
    let before = field(request, "latest").as_deref().and_then(ts_of);
    let (messages, complete) = world.history(&c, before, limit(request, "limit"));
    let mut out = page(messages, complete);
    out["ok"] = json!(true);
    out
}

/// A thread, as Slack pages it: forward from its own message, `limit` at a time, the next page
/// named by `response_metadata.next_cursor` (here the offset, spelled as Slack's opaque ones are).
fn replies(world: &World, request: &Logged) -> Value {
    let (Some(c), Some(ts)) = (channel(request), field(request, "ts").as_deref().and_then(ts_of)) else {
        return json!({"ok": false, "error": "thread_not_found"});
    };
    let Some((all, _)) = world.thread(&c, ts, None, usize::MAX) else {
        return json!({"ok": false, "error": "thread_not_found"});
    };
    let from: usize = field(request, "cursor")
        .and_then(|c| c.strip_prefix("dGhyZWFk:").and_then(|n| n.parse().ok()))
        .unwrap_or(0)
        .min(all.len());
    let to = (from + limit(request, "limit")).min(all.len());
    let list: Vec<Value> = all[from..to].iter().enumerate().map(|(n, m)| message(m, n)).collect();
    let more = to < all.len();
    let next = if more { format!("dGhyZWFk:{to}") } else { String::new() };
    json!({"ok": true, "messages": list, "has_more": more, "response_metadata": {"next_cursor": next}})
}

/// Do not disturb on now (from an hour ago for two hours).
fn dnd_on() -> Value {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    json!({"dnd_enabled": true, "next_dnd_start_ts": now - 3600, "next_dnd_end_ts": now + 3600, "snooze_enabled": false})
}

/// `dnd.teamInfo`: the people of `users` (comma-separated) the world knows, those in do not
/// disturb with it on now.
fn dnd_team(world: &World, request: &Logged) -> Value {
    let mut users = serde_json::Map::new();
    for id in field(request, "users").unwrap_or_default().split(',').filter(|id| !id.is_empty()) {
        let Some(u) = world.snapshot().users.iter().find(|u| u.id.as_str() == id) else { continue };
        let dnd = if u.presence == Presence::Dnd {
            dnd_on()
        } else {
            json!({"dnd_enabled": false, "next_dnd_start_ts": 1, "next_dnd_end_ts": 1})
        };
        users.insert(id.to_string(), dnd);
    }
    json!({"ok": true, "users": users})
}

/// A person of the world as Slack describes one; `None` for an id it does not know. Some leave
/// their display name out (Slack's empty display name).
fn person(world: &World, id: &str) -> Option<Value> {
    let u = world.snapshot().users.iter().find(|u| u.id.as_str() == id)?;
    let mut profile = json!({"real_name": u.display_name.unsanitized()});
    if id.len().is_multiple_of(2) {
        profile["display_name"] = json!(u.display_name.unsanitized());
    }
    Some(json!({
        "id": id,
        // A Slack Connect peer belongs to a team of its own.
        "team_id": match &u.org { Org::External(_) => "TPARTNER", Org::Own => u.workspace.as_str() },
        "name": u.name.unsanitized(),
        "real_name": u.display_name.unsanitized(),
        "is_bot": u.bot,
        "deleted": false,
        "profile": profile,
    }))
}

/// The edge cache's people lookup: `updated_ids` (id → version) in the JSON body; every id it
/// knows comes back, the others are listed as failed.
fn edge_users(world: &World, request: &Logged) -> Value {
    let body: Value = serde_json::from_str(&request.body).unwrap_or_default();
    let Some(ids) = body.get("updated_ids").and_then(Value::as_object) else {
        return json!({"ok": false, "error": "invalid_arguments"});
    };
    let (mut results, mut failed) = (Vec::new(), Vec::new());
    for id in ids.keys() {
        match person(world, id) {
            Some(p) => results.push(p),
            None => failed.push(json!(id)),
        }
    }
    json!({"ok": true, "results": results, "failed_ids": failed})
}
