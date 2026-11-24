//! The picture diagnostics: off unless [`DEBUG_ENV`] is `1`, then one line for each decision
//! taken about a picture, a workspace's emoji list, the user's own photo and the shape of the
//! sidebar's answers, in `<state>/pictures.log` ([`crate::paths::Paths::pictures_log`]), so a
//! run can tell where a picture stopped without anyone reading Slack's data.
//!
//! A line is `<RFC 3339 UTC time> <kind> <stage> <result> [key=value ...]` ([`Kind`],
//! [`Stage`]). It never holds a token, a cookie, an address (a host at most, never a path or a
//! query), a file or emoji name, someone's name, text or an id: a picture or a workspace is told
//! apart by `id=` / `ws=`, eight hex digits of a hash keyed anew each run ([`Diag::id`]). Every
//! value is cut to plain characters, and the whole line goes through the error log's redaction
//! ([`crate::fault::redact::redact`]) too.
//!
//! The file is written as the error log is (mode 0600 in a 0700 directory, never through a
//! link, [`LOG_MAX`] bytes then rotated once to `pictures.log.1`), by a thread of its own: a
//! line is handed over without waiting (one that finds the queue full is dropped), and a write
//! that fails is dropped too. Nothing here changes what slakio does.

use std::collections::HashSet;
use std::fmt;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The switch: `1` turns the diagnostics on for the run; anything else (or nothing) leaves
/// them off.
pub const DEBUG_ENV: &str = "SLAKIO_DEBUG_PICTURES";

/// The most bytes one diagnostics file holds before it is rotated.
pub const LOG_MAX: u64 = crate::fault::LOG_MAX;

/// The most lines waiting for the writer; past that a line is dropped.
const QUEUE: usize = 1024;

/// The most (kind, stage, id) told once ([`Diag::first`]) remembered; past that they are told
/// again.
const ONCE_MAX: usize = 16_384;

/// The most characters of one value.
const VALUE_MAX: usize = 64;

/// What a line is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A profile photo.
    Avatar,
    /// A smaller copy of a picture a message shares.
    Thumb,
    /// One of a workspace's own emoji pictures.
    Emoji,
    /// Where the user's own photo came from.
    SelfPhoto,
    /// A workspace's `emoji.list`.
    EmojiList,
    /// The shape of `users.channelSections.list`.
    Sections,
    /// The shape of `client.userBoot`.
    BootShape,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Avatar => "avatar",
            Kind::Thumb => "thumb",
            Kind::Emoji => "emoji",
            Kind::SelfPhoto => "self-photo",
            Kind::EmojiList => "emoji-list",
            Kind::Sections => "sections",
            Kind::BootShape => "boot-shape",
        }
    }

    /// The kind of a picture of `source`.
    pub fn of(source: &crate::images::Source) -> Self {
        match source {
            crate::images::Source::Thumb(_) => Kind::Thumb,
            crate::images::Source::Avatar(_) => Kind::Avatar,
            crate::images::Source::Emoji(_) => Kind::Emoji,
        }
    }
}

/// Where a decision was taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    /// An address (or a list) was read from Slack's answer.
    Parsed,
    /// An address was not taken (the result says why).
    AddressRefused,
    /// Wanted by a frame, not asked yet.
    Wanted,
    /// Asked for (a picture by the UI or on the wire, a method call).
    Asked,
    /// Not asked (the result says why).
    Skipped,
    /// A redirect: where to, and whether it is followed.
    Redirected,
    /// Refused after an answer came (the result says why).
    Refused,
    /// The HTTP status of an answer.
    Status,
    /// Slack's own `ok` / error code of an answer.
    Api,
    /// What an answer said it is (`type/subtype` only).
    ContentType,
    /// Counted: how many of something an answer had.
    Counted,
    /// Read into a picture (its size in pixels and format).
    Decoded,
    /// The governor's cap or share was reached.
    CapReached,
    /// Fetched and decoded, but the terminal's room is full.
    Parked,
    /// Shown on screen for the first time.
    Drawn,
    /// Taken in by the UI.
    Applied,
    /// Failed (the result says why).
    Failed,
    /// The source an answer came from.
    Source,
    /// A field of an answer: its name and what kind of value it holds.
    Field,
    /// Kept for the local picture cache: found there.
    CacheHit,
    /// Kept for the local picture cache: not there.
    CacheMiss,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Parsed => "parsed",
            Stage::AddressRefused => "address-refused",
            Stage::Wanted => "wanted",
            Stage::Asked => "asked",
            Stage::Skipped => "skipped",
            Stage::Redirected => "redirected",
            Stage::Refused => "refused",
            Stage::Status => "status",
            Stage::Api => "api",
            Stage::ContentType => "content-type",
            Stage::Counted => "counted",
            Stage::Decoded => "decoded",
            Stage::CapReached => "cap-reached",
            Stage::Parked => "parked",
            Stage::Drawn => "drawn",
            Stage::Applied => "applied",
            Stage::Failed => "failed",
            Stage::Source => "source",
            Stage::Field => "field",
            Stage::CacheHit => "cache-hit",
            Stage::CacheMiss => "cache-miss",
        }
    }
}

enum Note {
    Line(String),
    Flush(mpsc::Sender<()>),
}

struct Inner {
    path: PathBuf,
    tx: Option<SyncSender<Note>>,
    writer: Option<std::thread::JoinHandle<()>>,
    /// This run's key of [`Diag::id`].
    key: RandomState,
    once: Mutex<HashSet<u64>>,
}

impl Drop for Inner {
    // What was handed over is written before the run ends.
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
    }
}

/// The diagnostics' handle: off ([`Diag::default`]) or writing to one file. Cloned freely; every
/// clone writes to the same file, with the same key.
#[derive(Clone, Default)]
pub struct Diag(Option<Arc<Inner>>);

impl fmt::Debug for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.on() { "Diag(on)" } else { "Diag(off)" })
    }
}

impl Diag {
    /// On when `env` says [`DEBUG_ENV`] is `1` and there is a file to write (`path`).
    pub fn from_env(env: impl Fn(&str) -> Option<String>, path: Option<PathBuf>) -> Self {
        match (env(DEBUG_ENV).as_deref().map(str::trim), path) {
            (Some("1"), Some(path)) => Self::to(path),
            _ => Self::default(),
        }
    }

    /// On, writing to `path` (made only when the first line comes).
    pub fn to(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Note>(QUEUE);
        let at = path.clone();
        let writer = std::thread::Builder::new().name("slakio-diag".into()).spawn(move || {
            for note in rx {
                match note {
                    Note::Line(line) => {
                        let _ = crate::fault::append(&at, line.as_bytes(), LOG_MAX);
                    }
                    Note::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        // No thread: no diagnostics, and nothing else changes.
        let Ok(writer) = writer else { return Self::default() };
        let inner = Inner { path, tx: Some(tx), writer: Some(writer), key: RandomState::new(), once: Mutex::default() };
        Self(Some(Arc::new(inner)))
    }

    /// Whether lines are written.
    pub fn on(&self) -> bool {
        self.0.is_some()
    }

    /// Where they are written, when on.
    pub fn path(&self) -> Option<&Path> {
        self.0.as_deref().map(|i| i.path.as_path())
    }

    /// `raw` (an address, an id) as eight hex digits of this run's keyed hash: the same within a
    /// run, unrelated across runs, and never `raw` itself.
    pub fn id(&self, raw: &str) -> String {
        format!("{:08x}", self.hash(raw) >> 32)
    }

    fn hash(&self, raw: &str) -> u64 {
        let Some(inner) = &self.0 else { return 0 };
        let mut h = inner.key.build_hasher();
        h.write(raw.as_bytes());
        h.finish()
    }

    /// `true` the first time (`kind`, `stage`, `raw`) is asked about this run (when on): what is
    /// told once only.
    pub fn first(&self, kind: Kind, stage: Stage, raw: &str) -> bool {
        let Some(inner) = &self.0 else { return false };
        let mut h = inner.key.build_hasher();
        h.write(kind.as_str().as_bytes());
        h.write(stage.as_str().as_bytes());
        h.write(raw.as_bytes());
        let mut once = inner.once.lock().unwrap_or_else(PoisonError::into_inner);
        if once.len() >= ONCE_MAX {
            once.clear();
        }
        once.insert(h.finish())
    }

    /// Write one line (when on): `kind stage result`, then each `key=value` of `pairs`. Values
    /// are cut to plain characters ([`value`]); the line is redacted as the error log's is.
    pub fn line(&self, kind: Kind, stage: Stage, result: &str, pairs: &[(&str, &str)]) {
        let Some(inner) = &self.0 else { return };
        let Some(tx) = &inner.tx else { return };
        let mut line = format!("{} {} {} {}", rfc3339(SystemTime::now()), kind.as_str(), stage.as_str(), value(result));
        for (k, v) in pairs {
            line.push(' ');
            line.push_str(&value(k));
            line.push('=');
            line.push_str(&value(v));
        }
        let mut line = crate::fault::redact::redact(&line);
        line.push('\n');
        // Never waits: a full queue drops the line.
        let _ = tx.try_send(Note::Line(line));
    }

    /// Wait (at most two seconds) until what was handed over is written.
    pub fn flush(&self) {
        let Some(tx) = self.0.as_ref().and_then(|i| i.tx.as_ref()) else { return };
        let (done, wait) = mpsc::channel();
        if tx.send(Note::Flush(done)).is_ok() {
            let _ = wait.recv_timeout(Duration::from_secs(2));
        }
    }
}

/// `v` as a line may hold it: letters, digits and `. _ - / + : , *` only (anything else is `?`),
/// at most [`VALUE_MAX`] characters; empty is `-`.
pub fn value(v: &str) -> String {
    let mut out: String = v
        .chars()
        .take(VALUE_MAX)
        .map(|c| if c.is_ascii_alphanumeric() || "._-/+:,*".contains(c) { c } else { '?' })
        .collect();
    if out.is_empty() {
        out.push('-');
    }
    out
}

/// The host of `url` (`scheme://host…`), lowercase, without a port or a user; `relative` when it
/// names none, `odd` when the host has anything but letters, digits, `.` and `-`.
pub fn host(url: &str) -> String {
    let Some((_, rest)) = url.split_once("://") else { return "relative".to_string() };
    let end = rest.find(['/', '?', '#', '\\']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = authority.split(':').next().unwrap_or("").to_ascii_lowercase();
    let plain =
        !host.is_empty() && host.len() <= 253 && host.bytes().all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b));
    if plain { host } else { "odd".to_string() }
}

/// Why `source`'s address is no picture slakio fetches, in a word; `None` when it is one.
pub fn refusal(source: &crate::images::Source) -> Option<&'static str> {
    if source.checked().is_some() {
        return None;
    }
    let url = source.address();
    let Some(rest) = url.strip_prefix("https://") else { return Some("scheme") };
    if url.len() > crate::images::ADDRESS_MAX {
        return Some("length");
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let hosts: &[&str] = match source {
        crate::images::Source::Thumb(_) => &[crate::model::FILE_HOST],
        crate::images::Source::Avatar(_) => &crate::images::AVATAR_HOSTS,
        crate::images::Source::Emoji(_) => &crate::images::EMOJI_HOSTS,
    };
    if !hosts.contains(&host) {
        return Some("host");
    }
    if host == crate::images::AVATAR_HOSTS[1]
        && let Some(why) = crate::images::gravatar::refusal(url)
    {
        return Some(match why {
            "semicolon" => "gravatar-semicolon",
            "param" => "gravatar-param",
            "escape" => "gravatar-escape",
            _ => "gravatar-d-host",
        });
    }
    Some("charset")
}

/// For an address on Gravatar: the host of the fallback it names (`d=`, decoded) and whether
/// slakio takes that fallback; `None` for any other address or one that names none.
pub fn gravatar_fallback(url: &str) -> Option<(String, bool)> {
    let rest = url.strip_prefix("https://")?;
    if rest.split(['/', '?']).next() != Some(crate::images::AVATAR_HOSTS[1]) {
        return None;
    }
    let (value, ok) = crate::images::gravatar::fallback(url)?;
    let host = if value.contains("://") { host(&value) } else { "keyword".to_string() };
    Some((host, ok))
}

/// A short fixed word for the request governor's reason `why` (`per-minute`, `lane-picture`):
/// each reason is named by a word of its own, short enough that the log's redaction never takes
/// it for a secret; `other` for a reason not named here.
pub fn slug(why: &str) -> String {
    let words: &[(&str, &str)] = &[
        ("request cap of this run", "run-cap"),
        ("hourly request budget", "hourly-budget"),
        ("hourly background share", "hourly-background"),
        ("per-minute request cap", "per-minute"),
        ("pictures' share", "pictures"),
        ("workspace's background share", "lane-background"),
        ("workspace's picture share", "lane-picture"),
    ];
    words.iter().find(|(says, _)| why.contains(says)).map_or("other", |(_, word)| word).to_string()
}

/// `at` in RFC 3339, UTC, to the millisecond (`2026-01-05T09:00:00.000Z`).
pub fn rfc3339(at: SystemTime) -> String {
    let since = at.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil(i64::try_from(days).unwrap_or(0));
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        since.subsec_millis()
    )
}

/// The date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests;
