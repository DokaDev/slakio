//! Requests in the format of Slack's official web client: the browser's header set in its
//! order, the token in the form body (never an `Authorization` header), and the client's own
//! `_x_*` parameters in the order it sends them (by hand: a sorted query is not what a browser
//! sends).
//!
//! Derived from gammons/slk (MIT, Copyright (c) 2026 Grant Ammons): the header set and order,
//! the query and body parameter order, and the `_x_reason` / `_x_mode` tables
//! ([`Method::reason`], [`Method::sends_mode`]).

use crate::error::SlackError;
use crate::method::Method;
use serde_json::{Map, Value};
use slakio_core::secret::Credentials;
use std::net::SocketAddr;

/// The Chrome version the headers name; the client hints carry the same number.
const CHROME_MAJOR: &str = "150";

/// The `_x_version_ts` sent until Slack names a newer build.
pub const DEFAULT_VERSION_TS: &str = "1785403654";

/// Where requests go. This version reaches a server on this machine only (the tests' fake
/// Slack): there is no way to name another address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    api: String,
    edge: String,
    pub(crate) addr: SocketAddr,
}

impl Endpoints {
    /// A server on the loopback address `addr`; `None` for any other address.
    pub fn local(addr: SocketAddr) -> Option<Self> {
        addr.ip().is_loopback().then(|| Self {
            api: format!("http://{addr}/api/"),
            edge: format!("http://{addr}"),
            addr,
        })
    }
}

/// The operating system the browser headers claim, as the real one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        }
    }

    fn user_agent(self) -> String {
        let platform = match self {
            Os::Mac => "Macintosh; Intel Mac OS X 10_15_7",
            Os::Windows => "Windows NT 10.0; Win64; x64",
            Os::Linux => "X11; Linux x86_64",
        };
        format!(
            "Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{CHROME_MAJOR}.0.0.0 Safari/537.36"
        )
    }

    fn hint_platform(self) -> &'static str {
        match self {
            Os::Mac => "\"macOS\"",
            Os::Windows => "\"Windows\"",
            Os::Linux => "\"Linux\"",
        }
    }
}

/// Random ids of the request envelope (trace ids, the process's client and session ids).
pub trait Ids: Send {
    /// `bytes` random bytes as lowercase hex.
    fn hex(&mut self, bytes: usize) -> String;
}

/// Ids from the standard library's per-process random hash keys: unpredictable enough for
/// trace ids, which secure nothing.
#[derive(Debug, Default)]
pub struct RandomIds {
    n: u64,
}

impl Ids for RandomIds {
    fn hex(&mut self, bytes: usize) -> String {
        use std::hash::{BuildHasher, Hasher};
        let mut out = String::with_capacity(bytes * 2);
        while out.len() < bytes * 2 {
            self.n += 1;
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(self.n);
            out.push_str(&format!("{:016x}", h.finish()));
        }
        out.truncate(bytes * 2);
        out
    }
}

/// What the envelope keeps for the life of the process: the client id (`_x_id`'s prefix), the
/// session id (`_x_csid`), the team once signed in (`slack_route`) and Slack's build
/// (`_x_version_ts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    client_id: String,
    session_id: String,
    team: Option<String>,
    version_ts: String,
}

impl Session {
    pub fn new(ids: &mut dyn Ids) -> Self {
        // 8 hex characters, and 11 URL-safe characters (the length of 8 bytes in base64).
        let client_id = ids.hex(4);
        let session_id = ids.hex(6).chars().take(11).collect();
        Self { client_id, session_id, team: None, version_ts: DEFAULT_VERSION_TS.to_string() }
    }

    /// Signed in to `team`: the requests after it carry the post-boot parameters.
    pub fn set_team(&mut self, team: &str) {
        if !team.is_empty() {
            self.team = Some(team.to_string());
        }
    }

    pub fn set_version_ts(&mut self, ts: &str) {
        if !ts.is_empty() {
            self.version_ts = ts.to_string();
        }
    }
}

/// A call: an allowed method and its own parameters, in the order they are sent.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub method: Method,
    form: Vec<(String, String)>,
    json: Map<String, Value>,
}

impl Call {
    /// A workspace API call with form parameters.
    pub fn form(method: Method, params: &[(&str, &str)]) -> Self {
        Self {
            method,
            form: params.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect(),
            json: Map::new(),
        }
    }

    /// An edge cache call with a JSON body.
    pub fn json(method: Method, body: Map<String, Value>) -> Self {
        Self { method, form: Vec::new(), json: body }
    }
}

/// A request ready to send: everything the wire carries, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub url: String,
    /// In the order sent.
    pub headers: Vec<(&'static str, String)>,
    pub body: String,
}

/// The browser's headers of an API request (`Fetch` from `app.slack.com`), in order. No
/// `Referer`, no `Accept-Encoding`.
fn browser_headers(os: Os) -> Vec<(&'static str, String)> {
    let hint =
        format!("\"Not;A=Brand\";v=\"8\", \"Chromium\";v=\"{CHROME_MAJOR}\", \"Google Chrome\";v=\"{CHROME_MAJOR}\"");
    vec![
        ("User-Agent", os.user_agent()),
        ("Accept", "*/*".to_string()),
        ("Accept-Language", "en-US,en;q=0.9".to_string()),
        ("Origin", "https://app.slack.com".to_string()),
        ("Sec-Fetch-Site", "same-site".to_string()),
        ("Sec-Fetch-Mode", "cors".to_string()),
        ("Sec-Fetch-Dest", "empty".to_string()),
        ("Sec-Ch-Ua", hint),
        ("Sec-Ch-Ua-Mobile", "?0".to_string()),
        ("Sec-Ch-Ua-Platform", os.hint_platform().to_string()),
        ("Cache-Control", "no-cache".to_string()),
        ("Pragma", "no-cache".to_string()),
        ("Priority", "u=1, i".to_string()),
    ]
}

/// Build `call` for `credentials`, at `now_ms` (milliseconds since the Unix epoch).
pub fn prepare(
    call: &Call,
    credentials: &Credentials,
    session: &Session,
    endpoints: &Endpoints,
    now_ms: u64,
    ids: &mut dyn Ids,
    os: Os,
) -> Result<Prepared, SlackError> {
    let method = call.method;
    let mut query: Vec<(&str, String)> = Vec::new();
    let signed_in = session.team.is_some();
    let url = if method.edge() {
        let Some(team) = &session.team else {
            return Err(SlackError::Offline { why: "the edge cache needs a signed-in workspace" });
        };
        query.push(("_x_app_name", "client".to_string()));
        format!("{}/cache/{}/{}", endpoints.edge, encode(team), method.name())
    } else {
        let prefix = if signed_in { session.client_id.as_str() } else { "noversion" };
        query.push(("_x_id", format!("{prefix}-{}.{:03}", now_ms / 1000, now_ms % 1000)));
        if let Some(team) = &session.team {
            query.push(("_x_csid", session.session_id.clone()));
            query.push(("slack_route", team.clone()));
        }
        query.push(("_x_version_ts", session.version_ts.clone()));
        query.push(("_x_foreground", "true".to_string()));
        query.push(("_x_frontend_build_type", "current".to_string()));
        query.push(("_x_desktop_ia", "4".to_string()));
        query.push(("_x_gantry", "true".to_string()));
        format!("{}{}", endpoints.api, method.name())
    };
    if signed_in {
        query.push(("_x_b3_traceid", ids.hex(16)));
        query.push(("_x_b3_spanid", ids.hex(8)));
        query.push(("_x_b3_sampled", "1".to_string()));
    }
    query.push(("fp", "6e".to_string()));
    query.push(("_x_num_retries", "0".to_string()));
    let url = format!("{url}?{}", pairs(query.iter().map(|(k, v)| (*k, v.as_str()))));

    let (content_type, body) = if method.edge() {
        // The edge cache takes JSON as text, keys sorted, without any `_x_*` field.
        let mut body = call.json.clone();
        body.insert("token".to_string(), Value::String(credentials.token().to_string()));
        let sorted: std::collections::BTreeMap<_, _> = body.into_iter().collect();
        ("text/plain;charset=UTF-8", serde_json::to_string(&sorted).unwrap_or_default())
    } else {
        // The token first, then the call's own parameters, then the client's trailing fields.
        let mut fields: Vec<(&str, &str)> = vec![("token", credentials.token())];
        fields.extend(call.form.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        if let Some(reason) = method.reason() {
            fields.push(("_x_reason", reason));
        }
        if method.sends_mode() {
            fields.push(("_x_mode", "online"));
        }
        fields.push(("_x_sonic", "true"));
        fields.push(("_x_app_name", "client"));
        ("application/x-www-form-urlencoded", pairs(fields.into_iter()))
    };
    let mut headers = browser_headers(os);
    headers.push(("Content-Type", content_type.to_string()));
    headers.push(("Cookie", format!("d={}", credentials.cookie())));
    Ok(Prepared { url, headers, body })
}

/// `k=v&k=v`, form-encoded.
fn pairs<'a>(items: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    items.map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("&")
}

/// Form encoding (`application/x-www-form-urlencoded`): letters, digits and `-_.~` as they are,
/// a space as `+`, every other byte as `%XX`.
pub(crate) fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(char::from(b)),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests;
