//! A fake Slack for tests: an HTTP/1.1 server on `127.0.0.1:0`, in the test's own process,
//! that answers the read methods from the shared fake world ([`slakio_world::World`]) in the
//! shapes of Slack's answers, and logs every request exactly as it came off the wire (headers
//! in their order). Faults can be queued per method. Never a product dependency.
//!
//! The fake serves one workspace of the world, the first unless told another
//! ([`FakeSlack::start_team`]): two fakes serve a user signed in to two workspaces. Its answers add fields no decoder knows and
//! leave optional ones out now and then, so tolerant decoding is tested all the time.
//!
//! It also opens the live updates' socket ([`FakeSlack::push`] and the controls beside it); its
//! opening requests are kept apart from the API's ([`FakeSlack::upgrades`]).

mod shapes;
mod socket;

pub use socket::ClientFrame;

use slakio_world::World;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// A request as the server received it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Logged {
    /// The path without the query (`/api/client.userBoot`).
    pub path: String,
    pub query: String,
    /// In the order received, names as sent.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Logged {
    /// The method: the last part of an `/api/` path, or the edge cache's `kind/name`.
    pub fn method(&self) -> &str {
        match self.path.strip_prefix("/api/") {
            Some(m) => m,
            None => self.path.splitn(4, '/').nth(3).unwrap_or(&self.path),
        }
    }

    /// The form body's fields, in order (values left encoded).
    pub fn fields(&self) -> Vec<(&str, &str)> {
        self.body.split('&').filter_map(|kv| kv.split_once('=')).collect()
    }
}

/// What to answer instead of the world's data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// An HTTP status with an HTML body, and a `Retry-After` when given.
    Status { status: u16, retry_after: Option<String> },
    /// `{"ok": false, "error": code}`.
    Api(String),
    /// A body that is not JSON.
    Malformed,
    /// Never answer: the connection stays open until the client closes it
    /// ([`FakeSlack::hangs_closed`] counts those).
    Hang,
    /// This answer (a test's own synthetic one) instead of the world's.
    Answer(serde_json::Value),
    /// The world's own answer: a place in the queue, so the fault after it is the next request's.
    Pass,
}

#[derive(Default)]
struct Shared {
    log: Vec<Logged>,
    hangs_closed: usize,
    faults: HashMap<String, VecDeque<Fault>>,
    /// How long every answer waits before it is sent (a slow network).
    delay: std::time::Duration,
    /// The live updates sockets' opening requests, apart from [`Shared::log`] (the API's).
    upgrades: Vec<Logged>,
    /// The open sockets, to push to.
    sockets: Vec<tokio::sync::mpsc::UnboundedSender<socket::Push>>,
    /// How many of the next openings are refused (`503`, or `429` with this `Retry-After`).
    refuse_sockets: usize,
    refuse_retry_after: Option<String>,
    /// A socket says hello when it opens (the default).
    hello: bool,
    client_frames: Vec<ClientFrame>,
}

/// A running fake Slack; it stops when dropped.
pub struct FakeSlack {
    addr: SocketAddr,
    shared: Arc<Mutex<Shared>>,
    task: JoinHandle<()>,
}

impl Drop for FakeSlack {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeSlack {
    /// Start serving `world`'s first workspace (inside a Tokio runtime).
    pub async fn start(world: World) -> std::io::Result<Self> {
        Self::start_team(world, 0).await
    }

    /// Start serving `world`'s `team`-th workspace (inside a Tokio runtime).
    pub async fn start_team(world: World, team: usize) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Mutex::new(Shared { hello: true, ..Shared::default() }));
        let world = Arc::new(world);
        let task = {
            let shared = Arc::clone(&shared);
            tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(serve(stream, Arc::clone(&world), team, Arc::clone(&shared)));
                }
            })
        };
        Ok(Self { addr, shared, task })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Every request so far, in the order received.
    pub fn log(&self) -> Vec<Logged> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner).log.clone()
    }

    /// The methods asked for so far, in order.
    pub fn methods(&self) -> Vec<String> {
        self.log().iter().map(|l| l.method().to_string()).collect()
    }

    /// How many requests answered with [`Fault::Hang`] the client has closed.
    pub fn hangs_closed(&self) -> usize {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner).hangs_closed
    }

    /// Every answer from now on waits `delay` first (a slow network); zero for none.
    pub fn set_delay(&self, delay: std::time::Duration) {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner).delay = delay;
    }

    /// The live updates sockets asked for so far (their opening requests), refused ones too.
    pub fn upgrades(&self) -> Vec<Logged> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner).upgrades.clone()
    }

    /// The sockets open now.
    pub fn open_sockets(&self) -> usize {
        let mut s = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        s.sockets.retain(|t| !t.is_closed());
        s.sockets.len()
    }

    /// Send `event` (a synthetic one, as JSON) over every open socket.
    pub fn push(&self, event: &serde_json::Value) {
        self.to_sockets(&socket::Push::Text(event.to_string()));
    }

    /// Ping every open socket.
    pub fn ping(&self, payload: &[u8]) {
        self.to_sockets(&socket::Push::Ping(payload.to_vec()));
    }

    /// Close every open socket with a close frame.
    pub fn close_sockets(&self) {
        self.to_sockets(&socket::Push::Close);
    }

    /// End every open socket's connection without a word (the network went away).
    pub fn drop_sockets(&self) {
        self.to_sockets(&socket::Push::Drop);
    }

    /// Refuse the next `n` openings (`503`).
    pub fn refuse_sockets(&self, n: usize) {
        let mut s = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        (s.refuse_sockets, s.refuse_retry_after) = (n, None);
    }

    /// Refuse the next `n` openings as too many (`429`), asking to wait `retry_after` seconds.
    pub fn rate_limit_sockets(&self, n: usize, retry_after: &str) {
        let mut s = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        (s.refuse_sockets, s.refuse_retry_after) = (n, Some(retry_after.to_string()));
    }

    /// Whether a socket says hello when it opens (it does unless told otherwise).
    pub fn set_hello(&self, hello: bool) {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner).hello = hello;
    }

    /// Every frame the clients sent over their sockets, in order.
    pub fn client_frames(&self) -> Vec<ClientFrame> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner).client_frames.clone()
    }

    fn to_sockets(&self, push: &socket::Push) {
        let mut s = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        s.sockets.retain(|t| t.send(push.clone()).is_ok());
    }

    /// Answer the next request for `method` with `fault` (faults queue up per method).
    pub fn fail_next(&self, method: &str, fault: Fault) {
        let mut s = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        s.faults.entry(method.to_string()).or_default().push_back(fault);
    }
}

/// Read one request from `stream` and answer it.
async fn serve(mut stream: TcpStream, world: Arc<World>, team: usize, shared: Arc<Mutex<Shared>>) {
    let Some(logged) = read_request(&mut stream).await else { return };
    let upgrade =
        logged.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("upgrade") && v.eq_ignore_ascii_case("websocket"));
    if upgrade {
        return socket::serve(stream, logged, shared).await;
    }
    let (fault, delay) = {
        let mut s = shared.lock().unwrap_or_else(PoisonError::into_inner);
        s.log.push(logged.clone());
        (s.faults.get_mut(logged.method()).and_then(VecDeque::pop_front), s.delay)
    };
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let (status, extra, body) = match fault {
        Some(Fault::Hang) => {
            let mut byte = [0u8; 1];
            while stream.read(&mut byte).await.is_ok_and(|n| n > 0) {}
            shared.lock().unwrap_or_else(PoisonError::into_inner).hangs_closed += 1;
            return;
        }
        Some(Fault::Status { status, retry_after }) => {
            let extra = retry_after.map(|r| format!("retry-after: {r}\r\n")).unwrap_or_default();
            (status, extra, "<html><body>error</body></html>".to_string())
        }
        Some(Fault::Api(code)) => (200, String::new(), serde_json::json!({"ok": false, "error": code}).to_string()),
        Some(Fault::Malformed) => (200, String::new(), "{\"ok\": tr".to_string()),
        Some(Fault::Answer(v)) => (200, String::new(), v.to_string()),
        None | Some(Fault::Pass) => (200, String::new(), shapes::answer(&world, team, &logged).to_string()),
    };
    let head = format!(
        "HTTP/1.1 {status} FAKE\r\ncontent-type: application/json; charset=utf-8\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// The request line, headers and body (by `Content-Length`) of one HTTP/1.1 request.
async fn read_request(stream: &mut TcpStream) -> Option<Logged> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        let n = stream.read(&mut chunk).await.ok().filter(|n| *n > 0)?;
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8(buf[..head_end].to_vec()).ok()?;
    let mut lines = head.split("\r\n");
    let target = lines.next()?.split(' ').nth(1)?.to_string();
    let headers: Vec<(String, String)> =
        lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
    let length: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).await.ok().filter(|n| *n > 0)?;
        body.extend_from_slice(&chunk[..n]);
    }
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    Some(Logged {
        path: path.to_string(),
        query: query.to_string(),
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}
