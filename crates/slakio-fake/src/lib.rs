//! A fake Slack for tests: an HTTP/1.1 server on `127.0.0.1:0`, in the test's own process,
//! that answers the read methods from the shared fake world ([`slakio_world::World`]) in the
//! shapes of Slack's answers, and logs every request exactly as it came off the wire (headers
//! in their order). Faults can be queued per method. Never a product dependency.
//!
//! The fake serves one workspace of the world, the first unless told another
//! ([`FakeSlack::start_team`]): two fakes serve a user signed in to two workspaces. Its answers add fields no decoder knows and
//! leave optional ones out now and then, so tolerant decoding is tested all the time.

mod shapes;

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
}

#[derive(Default)]
struct Shared {
    log: Vec<Logged>,
    hangs_closed: usize,
    faults: HashMap<String, VecDeque<Fault>>,
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
        let shared = Arc::new(Mutex::new(Shared::default()));
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

    /// Answer the next request for `method` with `fault` (faults queue up per method).
    pub fn fail_next(&self, method: &str, fault: Fault) {
        let mut s = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        s.faults.entry(method.to_string()).or_default().push_back(fault);
    }
}

/// Read one request from `stream` and answer it.
async fn serve(mut stream: TcpStream, world: Arc<World>, team: usize, shared: Arc<Mutex<Shared>>) {
    let Some(logged) = read_request(&mut stream).await else { return };
    let fault = {
        let mut s = shared.lock().unwrap_or_else(PoisonError::into_inner);
        s.log.push(logged.clone());
        s.faults.get_mut(logged.method()).and_then(VecDeque::pop_front)
    };
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
        None => (200, String::new(), shapes::answer(&world, team, &logged).to_string()),
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
