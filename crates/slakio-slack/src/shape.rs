//! The Slack side of the picture diagnostics ([`slakio_core::diag`], off unless asked for): what
//! a workspace's `emoji.list` answered, where the user's own photo came from, which photo
//! addresses Slack's answers carried, and the shape (field names, counts and enum-like values,
//! never names, ids, text or addresses) of `client.userBoot`, `client.counts` and
//! `users.channelSections.list`. Nothing here changes what the backend does or answers.

use crate::error::SlackError;
use crate::method::Method;
use crate::response::Raw;
use crate::wire;
use serde_json::Value;
use slakio_core::diag::{Diag, Kind, Stage, host, refusal, slug};
use slakio_core::images::Source;
use std::collections::BTreeMap;

/// The most entries of one list described (sections of a page, DMs of the start bundle).
const ENTRIES_MAX: usize = 200;

/// The most field names of one object described.
const KEYS_MAX: usize = 40;

/// The longest field name or enum-like value told: under the 32 characters the log's redaction
/// takes for a secret, so what is told is never blanked.
const PLAIN_MAX: usize = 31;

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
    let plain = !k.is_empty()
        && k.len() <= PLAIN_MAX
        && k.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if plain { k } else { "other" }
}

/// An enum-like value (`standard`, `direct_messages`): lowercase letters, digits and `_`, at
/// most [`PLAIN_MAX`]; anything else (free text) is never told, only that it is `text`.
fn enum_like(v: &Value) -> String {
    match v.as_str() {
        Some(s)
            if !s.is_empty()
                && s.len() <= PLAIN_MAX
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
                // Not Slack's JSON: the call's failure says so (`failed parse`, `failed http`).
                _ => {}
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
        Method::ClientCounts => {
            let v: Value = serde_json::from_slice(&raw.body).unwrap_or_default();
            for (list, label) in [("channels", "channels_key"), ("mpims", "mpims_key"), ("ims", "ims_key")] {
                key_lines(diag, Kind::Counts, label, v.get(list).and_then(Value::as_array).into_iter().flatten(), "");
            }
        }
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
    sidebar_prefs(diag, v.get("prefs"));
    let profile = v.get("self").and_then(|s| s.get("profile"));
    diag.line(Kind::SelfPhoto, Stage::Parsed, "profile", &[("present", yn(profile.is_some_and(Value::is_object)))]);
    let images =
        profile.and_then(Value::as_object).into_iter().flat_map(|o| o.keys()).filter(|k| k.starts_with("image_"));
    for k in images.take(KEYS_MAX) {
        diag.line(Kind::SelfPhoto, Stage::Field, "image", &[("name", key(k))]);
    }
    key_lines(diag, Kind::SelfPhoto, "profile_key", profile.into_iter(), "");
    image_tally(diag, Kind::SelfPhoto, profile.into_iter());
}

/// The preferences whose names speak of the sidebar's order (`sort`, `sidebar`, `section`,
/// `order`, `dm`): each name, and its value when enum-like or a yes/no (else its kind).
fn sidebar_prefs(diag: &Diag, prefs: Option<&Value>) {
    const WORDS: [&str; 6] = ["sort", "sidebar", "channel_sort", "dm", "section", "order"];
    let named =
        prefs.and_then(Value::as_object).into_iter().flatten().filter(|(k, _)| WORDS.iter().any(|w| k.contains(w)));
    for (k, v) in named.take(KEYS_MAX) {
        let value = v.as_bool().map_or_else(|| enum_like(v), |b| b.to_string());
        diag.line(Kind::BootShape, Stage::Field, "pref", &[("name", key(k)), ("value", &value)]);
    }
}

/// The host of a photo address as a line may tell it: an avatar host, or Amazon's (Slack's own
/// uploads), else `other` (a host of a workspace's own is never told).
fn photo_host(url: &str) -> String {
    let h = host(url);
    let amazon = h.strip_suffix(".amazonaws.com").is_some_and(|s| !s.is_empty() && !s.contains('.'));
    if amazon || slakio_core::images::AVATAR_HOSTS.contains(&h.as_str()) { h } else { "other".to_string() }
}

/// What the `image_*` fields of `profiles` hold, told apart from their other names (sorted, those
/// may be cut at [`KEYS_MAX`]): how many have each, and by host ([`photo_host`], never a path)
/// how many of each are addresses; and how many carry `avatar_hash` and `is_custom_image` (and how many say `true`).
fn image_tally<'a>(diag: &Diag, kind: Kind, profiles: impl Iterator<Item = &'a Value>) {
    let (mut names, mut hosts) = (BTreeMap::<&str, usize>::new(), BTreeMap::<(&str, String), usize>::new());
    let (mut n, mut hash, mut custom, mut custom_true) = (0usize, 0usize, 0usize, 0usize);
    for o in profiles.take(ENTRIES_MAX).filter_map(Value::as_object) {
        n += 1;
        hash += usize::from(o.contains_key("avatar_hash"));
        custom += usize::from(o.contains_key("is_custom_image"));
        custom_true += usize::from(o.get("is_custom_image") == Some(&Value::Bool(true)));
        for (k, v) in o.iter().filter(|(k, _)| k.starts_with("image_")).take(KEYS_MAX) {
            *names.entry(key(k)).or_default() += 1;
            let Some(url) = v.as_str() else { continue };
            let h = photo_host(url);
            let h = if hosts.len() >= 8 && !hosts.contains_key(&(key(k), h.clone())) { "more".to_string() } else { h };
            *hosts.entry((key(k), h)).or_default() += 1;
        }
    }
    for (name, count) in names.into_iter().take(KEYS_MAX) {
        diag.line(kind, Stage::Counted, "image", &[("name", name), ("entries", &count.to_string())]);
    }
    for ((name, h), count) in hosts {
        diag.line(kind, Stage::Counted, "image_host", &[("name", name), ("host", &h), ("urls", &count.to_string())]);
    }
    let marks = [hash, custom, custom_true, n].map(|c| c.to_string());
    let pairs =
        [("avatar_hash", &*marks[0]), ("is_custom_image", &*marks[1]), ("custom_true", &*marks[2]), ("of", &*marks[3])];
    diag.line(kind, Stage::Counted, "marks", &pairs);
}

/// The photo addresses `persons` (from a view's `users`, or a lookup: `from`) carry: each one
/// taken, once a run, or why one is not.
pub(crate) fn photos(diag: &Diag, persons: &[wire::Person], from: &str) {
    if !diag.on() {
        return;
    }
    let (mut no_profile, mut no_photo, mut not_custom, mut pictured) = (0usize, 0usize, 0usize, 0usize);
    for p in persons.iter().take(ENTRIES_MAX) {
        let Some(profile) = &p.profile else {
            no_profile += 1;
            continue;
        };
        pictured += usize::from(profile.avatar().is_some());
        if profile.is_custom_image == Some(false) {
            not_custom += 1;
            continue;
        }
        let Some((field, url)) = profile.given() else {
            no_photo += 1;
            continue;
        };
        let source = Source::Avatar(url.to_string());
        let id = diag.id(url);
        let pairs = [("id", id.as_str()), ("host", &host(url)), ("field", field), ("from", from)];
        match (profile.photo(), refusal(&source)) {
            (Some((field, kept)), _) => {
                if diag.first(Kind::Avatar, Stage::Parsed, &kept) {
                    let id = diag.id(&kept);
                    let pairs = [("id", id.as_str()), ("host", &host(&kept)), ("from", from), ("field", field)];
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
    let counts = [persons.len(), pictured, no_profile, no_photo, not_custom].map(|c| c.to_string());
    let decoded = [("from", from), ("results", &*counts[0]), ("pictured", &*counts[1])];
    diag.line(Kind::Avatar, Stage::Counted, "decoded", &decoded);
    // Those without a photo: once a run for each tally (a skip is no longer silent).
    let skipped = no_profile + no_photo + not_custom;
    if skipped > 0 && diag.first(Kind::Avatar, Stage::Skipped, &format!("{from} {}", counts.join(" "))) {
        let (n, p, c, of) = (&*counts[2], &*counts[3], &*counts[4], &*counts[0]);
        let pairs = [("from", from), ("no_profile", n), ("no_photo", p), ("not_custom", c), ("of", of)];
        diag.line(Kind::Avatar, Stage::Skipped, "no-photo", &pairs);
    }
}

/// The user's own record (`me`) among a view's `persons`: once a run, whether it had a photo
/// slakio takes (`yes`), one it refuses (`refused`) or none (`no`).
pub(crate) fn own_seen(diag: &Diag, persons: &[wire::Person], me: Option<&slakio_core::model::UserId>) {
    if !diag.on() {
        return;
    }
    let Some(p) = me.and_then(|me| persons.iter().find(|p| p.id == me.as_str())) else { return };
    if !diag.first(Kind::SelfPhoto, Stage::Seen, "view") {
        return;
    }
    let raw = p.profile.as_ref().is_some_and(|f| f.given().is_some());
    let photo = match (p.profile.as_ref().and_then(wire::Profile::avatar), raw) {
        (Some(_), _) => "yes",
        (None, true) => "refused",
        (None, false) => "no",
    };
    diag.line(Kind::SelfPhoto, Stage::Seen, "ok", &[("from", "view"), ("photo", photo)]);
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
    let url = profile.given().map(|(_, u)| u);
    let id = url.map(|u| diag.id(u)).unwrap_or_default();
    match (profile.photo(), url) {
        (Some((field, kept)), _) => {
            let id = diag.id(&kept);
            let pairs = [("read", "yes"), ("photo", "ok"), ("host", &host(&kept)), ("id", &id), ("field", field)];
            diag.line(Kind::SelfPhoto, Stage::Parsed, "bundle", &pairs);
        }
        (None, _) if profile.is_custom_image == Some(false) => {
            diag.line(Kind::SelfPhoto, Stage::Parsed, "bundle", &[("read", "yes"), ("photo", "not_custom")]);
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
/// words), whether the answer itself had them (`answer`: with a photo taken or not) and whether
/// it gave a photo of them.
///
/// [`People::standing`]: crate::people::People::standing
pub(crate) fn own_lookup(diag: &Diag, standing: &str, answer: Option<bool>) {
    let photo = if answer == Some(true) { "ok" } else { "none" };
    diag.line(
        Kind::SelfPhoto,
        Stage::Lookup,
        "result",
        &[("standing", standing), ("in_results", yn(answer.is_some())), ("photo", photo)],
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
    let entries = || o.get("results").and_then(Value::as_array).into_iter().flatten();
    key_lines(diag, Kind::Avatar, "result_key", entries(), "");
    key_lines(diag, Kind::Avatar, "profile_key", entries().filter_map(|e| e.get("profile")), "");
    image_tally(diag, Kind::Avatar, entries().filter_map(|e| e.get("profile")));
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

/// A whole number as a line tells it, else what kind of value it is (`-` when absent).
fn number(v: Option<&Value>) -> String {
    v.map_or_else(|| "-".to_string(), |v| v.as_u64().map_or_else(|| type_of(v).to_string(), |n| n.to_string()))
}

/// One page's shape (`asked`: the cursor it was asked with): its fields, its `count`, whether
/// its top-level `cursor` is the one it was asked with, its `entities`, then each section's.
pub(crate) fn section_shape(diag: &Diag, v: &Value, page: &str, asked: Option<&str>) {
    key_lines(diag, Kind::Sections, "page_key", std::iter::once(v), page);
    let top = v.get("cursor").and_then(Value::as_str);
    let same = asked.map_or("-", |a| yn(top == Some(a)));
    let next = v.pointer("/response_metadata/next_cursor").and_then(Value::as_str).is_some_and(|c| !c.is_empty());
    let count = number(v.get("count"));
    let pairs = [
        ("page", page),
        ("count", &count),
        ("cursor", yn(top.is_some())),
        ("next_cursor", yn(next)),
        ("cursor_same", same),
    ];
    diag.line(Kind::Sections, Stage::Counted, "page", &pairs);
    entities(diag, v.get("entities"), page);
    let Some(list) = crate::sections::list(v) else {
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
        let ids_count = number(ids_page.and_then(|p| p.get("count")));
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
            ("ids_count", ids_count.as_str()),
        ];
        diag.line(Kind::Sections, Stage::Parsed, "section", &pairs);
        let sorts = s.as_object().into_iter().flatten().filter(|(k, _)| k.contains("sort") || k.contains("order"));
        for (k, value) in sorts.take(KEYS_MAX) {
            diag.line(
                Kind::Sections,
                Stage::Field,
                "sort",
                &[("page", page), ("n", &n), ("name", key(k)), ("value", &enum_like(value))],
            );
        }
    }
}

/// What a page's `entities` is: its type and size, the names of its fields (an id is `other`)
/// and of its entries' fields.
fn entities(diag: &Diag, v: Option<&Value>, page: &str) {
    let Some(v) = v else {
        return diag.line(Kind::Sections, Stage::Field, "entities", &[("page", page), ("type", "absent")]);
    };
    let (count, entries): (usize, Vec<&Value>) = match v {
        Value::Object(o) => (o.len(), o.values().collect()),
        Value::Array(a) => (a.len(), a.iter().collect()),
        _ => (0, Vec::new()),
    };
    let pairs = [("page", page), ("type", type_of(v)), ("count", &count.to_string())];
    diag.line(Kind::Sections, Stage::Field, "entities", &pairs);
    key_lines(diag, Kind::Sections, "entities_key", std::iter::once(v).filter(|v| v.is_object()), page);
    key_lines(diag, Kind::Sections, "entity_key", entries.into_iter(), page);
}

#[cfg(test)]
mod tests;
