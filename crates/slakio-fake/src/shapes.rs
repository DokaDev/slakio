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
        "search.messages" => search(world, team, request),
        "activity.feed" => activity(world, team, request),
        "messages.list" => messages_list(world, team, request),
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
    // Slack's bounds: after `oldest`, before `latest` (each message itself too when
    // `inclusive`); of those the newest `limit`, and whether more are left within them.
    let inclusive = field(request, "inclusive").as_deref() == Some("true");
    let bound = |key: &str, past: bool| {
        let t = field(request, key).as_deref().and_then(ts_of)?;
        Some(world.first_at_or_after(&c, if past { Ts(t.0 + 1) } else { t }))
    };
    let end = bound("latest", inclusive).unwrap_or_else(|| world.message_count(&c));
    let from = bound("oldest", !inclusive).unwrap_or(0);
    let start = end.saturating_sub(limit(request, "limit")).max(from);
    let messages = (start..end).filter_map(|i| world.message(&c, i)).collect();
    let mut out = page(messages, start <= from);
    out["ok"] = json!(true);
    out
}

/// A thread, as Slack pages it: forward from its own message, `limit` at a time, the next page
/// named by `response_metadata.next_cursor` (here the offset, spelled as Slack's opaque ones are).
/// With `oldest`, the replies from that time on (`inclusive`), its own message still first.
fn replies(world: &World, request: &Logged) -> Value {
    let (Some(c), Some(ts)) = (channel(request), field(request, "ts").as_deref().and_then(ts_of)) else {
        return json!({"ok": false, "error": "thread_not_found"});
    };
    let Some((mut all, _)) = world.thread(&c, ts, None, usize::MAX) else {
        return json!({"ok": false, "error": "thread_not_found"});
    };
    if let Some(oldest) = field(request, "oldest").as_deref().and_then(ts_of) {
        all.retain(|m| m.ts == ts || m.ts >= oldest);
    }
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

/// `search.messages`: page `page` of `count` (20 unless asked) of what the world's search finds
/// in the workspace served, newest first; every third match leaves its author's handle out.
fn search(world: &World, team: usize, request: &Logged) -> Value {
    let Some(ws) = served(world, team) else { return json!({"ok": false, "error": "invalid_auth"}) };
    let query = field(request, "query").unwrap_or_default();
    let count = field(request, "count").and_then(|v| v.parse::<usize>().ok()).unwrap_or(20).clamp(1, 100);
    let page = field(request, "page").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).max(1);
    let all = world.search(&ws.id, &query);
    let pages = all.len().div_ceil(count);
    let s = world.snapshot();
    let host = format!("{}.slack.com", ws.id.as_str().to_ascii_lowercase());
    let matches: Vec<Value> = (all.iter().skip((page - 1) * count).take(count).enumerate())
        .map(|(n, h)| {
            let c = s.conversations.iter().find(|c| c.id == h.conversation);
            let p = format!("p{}{:06}", h.message.ts.secs(), h.message.ts.0 % 1_000_000);
            let mut link = format!("https://{host}/archives/{}/{p}", h.conversation.as_str());
            if let Some(t) = h.thread {
                link.push_str(&format!("?thread_ts={t}&cid={}", h.conversation.as_str()));
            }
            let mut m = json!({
                "iid": format!("iid-{n}"),
                "type": "message",
                "team": ws.id.as_str(),
                "channel": {"id": h.conversation.as_str(), "name": c.map(|c| c.name.unsanitized()).unwrap_or_default(), "is_private": false},
                "user": h.message.user.as_str(),
                "ts": h.message.ts.to_string(),
                "text": h.message.text.unsanitized(),
                "permalink": link,
            });
            if n % 3 != 0 {
                let handle = s.users.iter().find(|u| u.id == h.message.user).map(|u| u.name.unsanitized());
                m["username"] = json!(handle.unwrap_or_default());
            }
            m
        })
        .collect();
    let paging = json!({"count": count, "total": all.len(), "page": page, "pages": pages});
    json!({"ok": true, "query": query, "messages": {"total": all.len(), "paging": paging, "pagination": {"page": page, "page_count": pages}, "matches": matches}})
}

/// `activity.feed`: `limit` (20 unless asked) of the world's notices of the workspace served,
/// newest first, from the cursor (`c<offset>`) on, each in the shape of its kind; the next
/// cursor while more are left, an empty one at the end (as Slack ends a list).
fn activity(world: &World, team: usize, request: &Logged) -> Value {
    let Some(ws) = served(world, team) else { return json!({"ok": false, "error": "invalid_auth"}) };
    let all = world.activity(&ws.id);
    let limit = field(request, "limit").and_then(|v| v.parse::<usize>().ok()).unwrap_or(20).clamp(1, 100);
    let from = field(request, "cursor").and_then(|c| c.strip_prefix('c')?.parse::<usize>().ok()).unwrap_or(0);
    let me = ws.me.as_str();
    let items: Vec<Value> = (all.iter().skip(from).take(limit))
        .map(|n| {
            let (ts, channel) = (n.message.ts.to_string(), n.conversation.as_str());
            let mut message = json!({"ts": ts, "channel": channel, "author_user_id": n.message.user.as_str()});
            if let Some(t) = n.thread {
                message["thread_ts"] = json!(t.to_string());
            }
            let mut item = json!({"type": n.kind});
            match n.kind {
                "thread_v2" => {
                    let entry =
                        json!({"channel_id": channel, "thread_ts": n.thread.map(|t| t.to_string()), "latest_ts": ts});
                    item["bundle_info"] = json!({"unread_count": 2, "payload": {"thread_entry": entry}});
                }
                "dm" => item["bundle_info"] = json!({"payload": {"dm_entry": {"latest_message": message}}}),
                "message_reaction" => {
                    message["author_user_id"] = json!(me);
                    item["message"] = message;
                    let (who, name) = n.reactor.as_ref().map_or(("", ""), |(u, r)| (u.as_str(), *r));
                    item["reaction"] = json!({"user": who, "name": name});
                }
                _ => item["message"] = message,
            }
            json!({"key": n.key, "feed_ts": n.at.to_string(), "is_unread": n.unread, "item": item})
        })
        .collect();
    let next = if from + limit < all.len() { format!("c{}", from + limit) } else { String::new() };
    json!({"ok": true, "items": items, "response_metadata": {"next_cursor": next}})
}

/// `messages.list`: the messages `message_ids` names (`[{"channel", "timestamps"}]`) that the
/// world has, by conversation; one it does not have is left out.
fn messages_list(world: &World, team: usize, request: &Logged) -> Value {
    let Some(ws) = served(world, team) else { return json!({"ok": false, "error": "invalid_auth"}) };
    let ids: Value = field(request, "message_ids").and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
    let notices = world.activity(&ws.id);
    let mut data = serde_json::Map::new();
    for group in ids.as_array().into_iter().flatten() {
        let Some(channel) = group["channel"].as_str() else { continue };
        let id = ConversationId::new(channel);
        let found: Vec<Value> = (group["timestamps"].as_array().into_iter().flatten())
            .filter_map(|t| ts_of(t.as_str()?))
            .filter_map(|ts| {
                let noticed = notices.iter().find(|n| n.conversation == id && n.message.ts == ts);
                noticed.map(|n| n.message.clone()).or_else(|| world.message(&id, world.index_of(&id, ts)?))
            })
            .enumerate()
            .map(|(n, m)| message(&m, n))
            .collect();
        data.insert(channel.to_string(), json!({"messages": found}));
    }
    json!({"ok": true, "messages_data": data})
}
