//! The Slack side of the picture diagnostics ([`slakio_core::diag`], off unless asked for): what
//! a workspace's `emoji.list` answered, where the user's own photo came from, which photo
//! addresses Slack's answers carried, and the shape (field names, counts and enum-like values,
//! never names, ids, text or addresses) of `client.userBoot` and `users.channelSections.list`.
//! Nothing here changes what the backend does or answers: the sections are read only to be
//! described, once a workspace has booted, and only while the diagnostics are on.

use crate::client::Client;
use crate::error::SlackError;
use crate::governor::Priority;
use crate::method::Method;
use crate::request::Call;
use crate::response::Raw;
use crate::wire;
use serde_json::Value;
use slakio_core::diag::{Diag, Kind, Stage, host, refusal, slug};
use slakio_core::images::Source;
use std::collections::BTreeMap;

/// The most pages of `users.channelSections.list` the probe reads.
pub const SECTION_PAGES: usize = 5;

/// The most entries of one list described (sections of a page, DMs of the start bundle).
const ENTRIES_MAX: usize = 200;

/// The most field names of one object described.
const KEYS_MAX: usize = 40;

/// `yes` / `no`.
fn yn(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// What kind of value `v` is.
fn type_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// A field's name as a line may hold it: a plain `snake_case` name, else `other`.
fn key(k: &str) -> &str {
    let plain =
        !k.is_empty() && k.len() <= 40 && k.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if plain { k } else { "other" }
}

/// An enum-like value (`standard`, `direct_messages`): lowercase letters, digits and `_`, at
/// most 32; anything else (free text) is never told, only that it is `text`.
fn enum_like(v: &Value) -> String {
    match v.as_str() {
        Some(s)
            if !s.is_empty()
                && s.len() <= 32
                && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') =>
        {
            s.to_string()
        }
        Some(_) => "text".to_string(),
        None => type_of(v).to_string(),
    }
}

/// One line per field name the objects of `entries` have (at most [`ENTRIES_MAX`] of them read,
/// [`KEYS_MAX`] names each): `label`, the name and how many of them have it.
fn key_lines<'a>(diag: &Diag, kind: Kind, label: &str, entries: impl Iterator<Item = &'a Value>, page: &str) {
    let mut names: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in entries.take(ENTRIES_MAX) {
        for k in entry.as_object().into_iter().flat_map(|o| o.keys()).take(KEYS_MAX) {
            *names.entry(key(k)).or_default() += 1;
        }
    }
    for (name, n) in names.into_iter().take(KEYS_MAX) {
        let n = n.to_string();
        let pairs = [("name", name), ("entries", n.as_str()), ("page", page)];
        diag.line(kind, Stage::Field, label, if page.is_empty() { &pairs[..2] } else { &pairs });
    }
}

/// Why `e` failed, in a word.
pub(crate) fn reason(e: &SlackError) -> String {
    match e {
        SlackError::CapReached { why } => slug(why),
        SlackError::Api { code } => format!("api-{code}"),
        SlackError::Auth { .. } => "signed-out".to_string(),
        SlackError::RateLimited { .. } => "rate-limited".to_string(),
        SlackError::Offline { .. } => "offline".to_string(),
        SlackError::Network { detail } if detail.contains("JSON") => "parse".to_string(),
        SlackError::Network { detail } if detail.contains("too long") || detail.contains("timed out") => {
            "timeout".to_string()
        }
        SlackError::Network { detail } if detail.contains("cut off") => "cut-off".to_string(),
        SlackError::Network { detail } if detail.starts_with("HTTP") => "http".to_string(),
        SlackError::Network { .. } => "network".to_string(),
        SlackError::TooLarge => "too-large".to_string(),
        SlackError::Busy => "busy".to_string(),
        _ => "other".to_string(),
    }
}

/// `e` as a line of `kind`: the governor's cap as [`Stage::CapReached`] (which share, of the
/// workspace or the process), anything else as [`Stage::Failed`].
pub(crate) fn failed(diag: &Diag, kind: Kind, e: &SlackError, pairs: &[(&str, &str)]) {
    let mut all = pairs.to_vec();
    match e {
        SlackError::CapReached { why } => {
            all.push(("lane", if why.contains("workspace") { "workspace" } else { "process" }));
            diag.line(kind, Stage::CapReached, &slug(why), &all);
        }
        e => diag.line(kind, Stage::Failed, &reason(e), &all),
    }
}

/// A call of `method` is about to go out (`signed_in`: with the post-boot parameters).
pub(crate) fn sending(diag: &Diag, method: Method, signed_in: bool) {
    if diag.on() && method == Method::EmojiList {
        diag.line(Kind::EmojiList, Stage::Asked, "ok", &[("after_boot", yn(signed_in))]);
    }
}

/// An answer to `method` came off the wire: its status (and Slack's `ok` and error code) for
/// `emoji.list`, the shape of `client.userBoot`.
pub(crate) fn answered(diag: &Diag, method: Method, raw: &Raw) {
    if !diag.on() {
        return;
    }
    match method {
        Method::EmojiList => {
            diag.line(Kind::EmojiList, Stage::Status, &raw.status.to_string(), &[]);
            let v: Option<Value> = serde_json::from_slice(&raw.body).ok();
            match v.as_ref().map(|v| (v.get("ok").and_then(Value::as_bool), v.get("error"))) {
                Some((Some(true), _)) => diag.line(Kind::EmojiList, Stage::Api, "ok", &[]),
                Some((Some(false), code)) => {
                    let code = code.map_or_else(|| "no_error_code".to_string(), enum_like);
                    diag.line(Kind::EmojiList, Stage::Api, "error", &[("code", &code)]);
                }
                _ => diag.line(Kind::EmojiList, Stage::Failed, "parse", &[]),
            }
        }
        Method::EdgeUsersInfo => match serde_json::from_slice::<Value>(&raw.body) {
            Ok(v) => lookup_shape(diag, &v),
            Err(_) => diag.line(Kind::Avatar, Stage::Failed, "parse", &[("lookup", "users-info")]),
        },
        Method::ClientUserBoot => match serde_json::from_slice::<Value>(&raw.body) {
            Ok(v) => boot_shape(diag, &v),
            Err(_) => diag.line(Kind::BootShape, Stage::Failed, "parse", &[]),
        },
        _ => {}
    }
}

/// The start bundle's shape: whether it carries `starred`, `is_open`, `channel_sections` (and
/// how many entries), the field names of its DMs and of the user's own profile's pictures.
fn boot_shape(diag: &Diag, v: &Value) {
    if !v.is_object() {
        return diag.line(Kind::BootShape, Stage::Failed, "parse", &[]);
    }
    for name in ["starred", "is_open", "channel_sections", "ims", "channels"] {
        let Some(f) = v.get(name) else {
            diag.line(Kind::BootShape, Stage::Field, "absent", &[("name", name)]);
            continue;
        };
        let count = f.as_array().map_or_else(|| "-".to_string(), |a| a.len().to_string());
        diag.line(Kind::BootShape, Stage::Field, "present", &[("name", name), ("type", type_of(f)), ("count", &count)]);
    }
    for (list, label) in [("ims", "ims_key"), ("channel_sections", "section_key")] {
        key_lines(diag, Kind::BootShape, label, v.get(list).and_then(Value::as_array).into_iter().flatten(), "");
    }
    let profile = v.get("self").and_then(|s| s.get("profile"));
    diag.line(Kind::SelfPhoto, Stage::Parsed, "profile", &[("present", yn(profile.is_some_and(Value::is_object)))]);
    for k in profile.and_then(Value::as_object).into_iter().flat_map(|o| o.keys()).filter(|k| k.starts_with("image_")) {
        diag.line(Kind::SelfPhoto, Stage::Field, "image", &[("name", key(k))]);
    }
}

/// The photo addresses `persons` (from a view's `users`, or a lookup: `from`) carry: each one
/// taken, once a run, or why one is not.
pub(crate) fn photos(diag: &Diag, persons: &[wire::Person], from: &str) {
    if !diag.on() {
        return;
    }
    for p in persons.iter().take(ENTRIES_MAX) {
        let Some(profile) = &p.profile else { continue };
        let fields = [("image_72", &profile.image_72), ("image_48", &profile.image_48)];
        let Some((field, url)) = fields.iter().find_map(|(f, u)| u.as_deref().map(|u| (*f, u))) else { continue };
        let source = Source::Avatar(url.to_string());
        let id = diag.id(url);
        let pairs = [("id", id.as_str()), ("host", &host(url)), ("field", field), ("from", from)];
        match (profile.avatar(), refusal(&source)) {
            (Some(kept), _) => {
                if diag.first(Kind::Avatar, Stage::Parsed, &kept) {
                    let id = diag.id(&kept);
                    let pairs = [("id", id.as_str()), ("host", &host(&kept)), ("from", from)];
                    diag.line(Kind::Avatar, Stage::Parsed, "ok", &pairs);
                }
            }
            (None, why) => {
                if diag.first(Kind::Avatar, Stage::AddressRefused, url) {
                    diag.line(Kind::Avatar, Stage::AddressRefused, why.unwrap_or("other"), &pairs);
                }
            }
        }
    }
}

/// The user's own photo in the start bundle: whether its profile was read, and its photo
/// taken (and from which host) or why not.
pub(crate) fn own_profile(diag: &Diag, me: &wire::Me) {
    if !diag.on() {
        return;
    }
    let Some(profile) = &me.profile else {
        return diag.line(Kind::SelfPhoto, Stage::Parsed, "bundle", &[("read", "no")]);
    };
    let url = profile.image_72.as_deref().or(profile.image_48.as_deref());
    let id = url.map(|u| diag.id(u)).unwrap_or_default();
    match (profile.avatar(), url) {
        (Some(kept), _) => {
            let pairs = [("read", "yes"), ("photo", "ok"), ("host", &host(&kept)), ("id", &diag.id(&kept))];
            diag.line(Kind::SelfPhoto, Stage::Parsed, "bundle", &pairs);
        }
        (None, Some(u)) => {
            let why = refusal(&Source::Avatar(u.to_string())).unwrap_or("other");
            let pairs = [("read", "yes"), ("photo", "refused"), ("why", why), ("host", &host(u)), ("id", &id)];
            diag.line(Kind::SelfPhoto, Stage::AddressRefused, "bundle", &pairs);
        }
        (None, None) => diag.line(Kind::SelfPhoto, Stage::Parsed, "bundle", &[("read", "yes"), ("photo", "none")]),
    }
}

/// What the lookup gave of the user's own record: where they stand after it ([`People::standing`]
/// words), whether the answer named them and whether a photo of them is known now.
///
/// [`People::standing`]: crate::people::People::standing
pub(crate) fn own_lookup(diag: &Diag, standing: &str) {
    let named = matches!(standing, "pictured" | "unpictured");
    let photo = if standing == "pictured" { "ok" } else { "none" };
    diag.line(
        Kind::SelfPhoto,
        Stage::Lookup,
        "result",
        &[("standing", standing), ("in_results", yn(named)), ("photo", photo)],
    );
}

/// The shape of an edge `users/info` answer: its top-level field names, and how many ids it
/// gave (`results`), has no answer for yet (`pending_ids`) or does not know (`failed_ids`).
fn lookup_shape(diag: &Diag, v: &Value) {
    let Some(o) = v.as_object() else {
        return diag.line(Kind::Avatar, Stage::Failed, "parse", &[("lookup", "users-info")]);
    };
    for k in o.keys().take(KEYS_MAX) {
        diag.line(Kind::Avatar, Stage::Field, "lookup_key", &[("name", key(k))]);
    }
    let count =
        |name: &str| o.get(name).and_then(Value::as_array).map_or_else(|| "-".to_string(), |a| a.len().to_string());
    let (results, pending, failed) = (count("results"), count("pending_ids"), count("failed_ids"));
    let pairs = [("results", results.as_str()), ("pending_ids", pending.as_str()), ("failed_ids", failed.as_str())];
    diag.line(Kind::Avatar, Stage::Counted, "lookup", &pairs);
}

/// Where the user's own photo came from: the start bundle's `profile`, a page read before the
/// boot (`page`), the edge lookup (`lookup`), or nowhere (`none`).
pub(crate) fn own_source(diag: &Diag, source: &str, photo: Option<&str>) {
    if !diag.on() {
        return;
    }
    let (host, id) = photo.map_or_else(|| ("-".to_string(), "-".to_string()), |p| (host(p), diag.id(p)));
    diag.line(Kind::SelfPhoto, Stage::Source, source, &[("host", &host), ("id", &id)]);
}

/// What `emoji.list` gave: names read, kept and dropped; picture addresses by host, and how many
/// of each host are refused; what they resolve to.
pub(crate) fn emoji_counts(
    diag: &Diag,
    read: usize,
    kept: &[(String, String)],
    resolved: &[(String, slakio_core::images::emoji::Custom)],
) {
    if !diag.on() {
        return;
    }
    let (n, k) = (read.to_string(), kept.len().to_string());
    let dropped = read.saturating_sub(kept.len()).to_string();
    diag.line(Kind::EmojiList, Stage::Counted, "names", &[("read", &n), ("kept", &k), ("dropped", &dropped)]);
    let mut hosts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (_, value) in kept.iter().filter(|(_, v)| !v.starts_with("alias:")) {
        let refused = slakio_core::images::emoji_address(value).is_none();
        let h = if hosts.len() >= 8 && !hosts.contains_key(&host(value)) { "more".to_string() } else { host(value) };
        let e = hosts.entry(h).or_default();
        e.0 += 1;
        e.1 += usize::from(refused);
    }
    for (h, (urls, refused)) in hosts {
        diag.line(
            Kind::EmojiList,
            Stage::Counted,
            "host",
            &[("host", &h), ("urls", &urls.to_string()), ("refused", &refused.to_string())],
        );
    }
    let pictures = resolved.iter().filter(|(_, c)| matches!(c, slakio_core::images::emoji::Custom::Picture(_))).count();
    let standard = (resolved.len() - pictures).to_string();
    diag.line(
        Kind::EmojiList,
        Stage::Counted,
        "resolved",
        &[("pictures", &pictures.to_string()), ("standard", &standard)],
    );
}

/// Once a workspace has booted, with the diagnostics on: the shape of its sidebar's sections
/// (`users.channelSections.list`, read-only, in the background share), a page at a time, at most
/// [`SECTION_PAGES`] pages. Nothing read is kept.
pub(crate) async fn sections(client: &Client) {
    let diag = client.diag();
    if !diag.on() {
        return;
    }
    let mut cursor: Option<String> = None;
    for page in 1..=SECTION_PAGES {
        let n = page.to_string();
        let params: Vec<(&str, &str)> = cursor.as_deref().map(|c| ("cursor", c)).into_iter().collect();
        diag.line(Kind::Sections, Stage::Asked, "ok", &[("page", &n), ("cursor", yn(cursor.is_some()))]);
        let call = Call::form(Method::UsersChannelSectionsList, &params);
        let v = match client.call::<Value>(&call, Priority::Background).await {
            Ok(v) => v,
            Err(e) => return failed(diag, Kind::Sections, &e, &[("page", &n)]),
        };
        section_shape(diag, &v, &n);
        cursor = next_cursor(&v);
        if cursor.is_none() {
            return diag.line(Kind::Sections, Stage::Counted, "done", &[("pages", &n)]);
        }
    }
    diag.line(Kind::Sections, Stage::Counted, "page-cap", &[("pages", &SECTION_PAGES.to_string())]);
}

/// The cursor of the next page, when the answer names one.
fn next_cursor(v: &Value) -> Option<String> {
    [v.pointer("/response_metadata/next_cursor"), v.get("next_cursor"), v.get("cursor")]
        .into_iter()
        .flatten()
        .find_map(Value::as_str)
        .filter(|c| !c.is_empty())
        .map(str::to_string)
}

/// One page's shape: its fields, then each section's.
fn section_shape(diag: &Diag, v: &Value, page: &str) {
    key_lines(diag, Kind::Sections, "page_key", std::iter::once(v), page);
    let list = ["channel_sections", "sections"].into_iter().find_map(|k| v.get(k).and_then(Value::as_array));
    let Some(list) = list else {
        return diag.line(Kind::Sections, Stage::Failed, "parse", &[("page", page)]);
    };
    diag.line(Kind::Sections, Stage::Counted, "sections", &[("page", page), ("count", &list.len().to_string())]);
    key_lines(diag, Kind::Sections, "section_key", list.iter(), page);
    for (i, s) in list.iter().take(ENTRIES_MAX).enumerate() {
        let n = i.to_string();
        let kind = s.get("type").map_or_else(|| "absent".to_string(), enum_like);
        let name_len = s.get("name").and_then(Value::as_str).map_or(0, |t| t.chars().count()).to_string();
        let ids_page = s.get("channel_ids_page");
        let ids = ids_page.and_then(|p| p.get("channel_ids")).and_then(Value::as_array).map_or(0, Vec::len).to_string();
        let ids_cursor = ids_page.and_then(|p| p.get("cursor")).and_then(Value::as_str).is_some_and(|c| !c.is_empty());
        let next = s.get("next_channel_section_id").is_some_and(|n| !n.is_null());
        let pairs = [
            ("page", page),
            ("n", n.as_str()),
            ("type", kind.as_str()),
            ("name_len", name_len.as_str()),
            ("is_custom", yn(kind == "standard")),
            ("ids", ids.as_str()),
            ("ids_page", yn(ids_page.is_some())),
            ("ids_cursor", yn(ids_cursor)),
            ("next_id", yn(next)),
        ];
        diag.line(Kind::Sections, Stage::Parsed, "section", &pairs);
        for (k, value) in s.as_object().into_iter().flatten().filter(|(k, _)| k.contains("sort") || k.contains("order"))
        {
            diag.line(
                Kind::Sections,
                Stage::Field,
                "sort",
                &[("n", &n), ("name", key(k)), ("value", &enum_like(value))],
            );
        }
    }
}

#[cfg(test)]
mod tests;
