//! A file's download on the wire: one `GET` to Slack's file host ([`FILE_HOST`]) and no
//! other, over TLS (HTTP/2 when offered, else HTTP/1.1), or for the tests' fake Slack to its
//! loopback address (the file host's path, sent there). The answer's head is read at once; its
//! body comes a piece at a time ([`Fetched::chunk`]), each within [`IDLE`] of the one before.
//! A redirect is not followed here: the caller decides whether its address may be fetched.

use super::{AbortOnDrop, config, net, split};
use crate::error::SlackError;
use crate::request::{Endpoints, Server};
use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper_util::rt::{TokioExecutor, TokioIo};
use slakio_core::model::FILE_HOST;
use std::time::Duration;
use tokio::net::TcpStream;

/// The longest a download waits for its answer, and then for each piece of it.
pub const IDLE: Duration = Duration::from_secs(30);

const ONLY_FILES: SlackError = SlackError::Offline { why: "only Slack's file host can be reached for a file" };

/// The head of a file's answer, and its body still to read.
pub(crate) struct Fetched {
    pub(crate) status: u16,
    /// Where a redirect sends it.
    pub(crate) location: Option<String>,
    pub(crate) content_type: Option<String>,
    /// The length said, when it was.
    pub(crate) length: Option<u64>,
    pub(crate) retry_after: Option<String>,
    body: hyper::body::Incoming,
    /// The connection's task: it ends when this is dropped.
    _conn: AbortOnDrop<()>,
}

impl Fetched {
    /// The body's next piece; `None` at its end.
    pub(crate) async fn chunk(&mut self) -> Result<Option<Bytes>, SlackError> {
        loop {
            let frame = match tokio::time::timeout(IDLE, self.body.frame()).await {
                Err(_) => return Err(net("the download stalled")),
                Ok(None) => return Ok(None),
                Ok(Some(Err(_))) => return Err(net("the download was cut off")),
                Ok(Some(Ok(frame))) => frame,
            };
            // Trailers carry no bytes of the file.
            if let Ok(data) = frame.into_data() {
                return Ok(Some(data));
            }
        }
    }
}

/// `GET` `url` (an address on the file host) with `headers` in order.
pub(crate) async fn get(
    endpoints: &Endpoints,
    url: &str,
    headers: &[(&'static str, String)],
) -> Result<Fetched, SlackError> {
    match tokio::time::timeout(IDLE, head(endpoints, url, headers)).await {
        Ok(answer) => answer,
        Err(_) => Err(net("timed out")),
    }
}

async fn head(endpoints: &Endpoints, url: &str, headers: &[(&'static str, String)]) -> Result<Fetched, SlackError> {
    let (host, path) = split(url).ok_or(ONLY_FILES)?;
    if host != FILE_HOST {
        return Err(ONLY_FILES);
    }
    match &endpoints.server {
        Server::Local(addr) => {
            if !addr.ip().is_loopback() {
                return Err(ONLY_FILES);
            }
            let stream = TcpStream::connect(addr).await.map_err(|_| net("could not connect"))?;
            http1(TokioIo::new(stream), &addr.to_string(), path, headers).await
        }
        Server::Slack { .. } => {
            let tcp = TcpStream::connect((FILE_HOST, 443)).await.map_err(|_| net("could not connect"))?;
            let name = rustls::pki_types::ServerName::try_from(FILE_HOST).map_err(|_| ONLY_FILES)?;
            let tls = tokio_rustls::TlsConnector::from(config()?)
                .connect(name, tcp)
                .await
                .map_err(|_| net("the secure connection failed"))?;
            if tls.get_ref().1.alpn_protocol() == Some(b"h2") {
                http2(TokioIo::new(tls), &format!("https://{FILE_HOST}{path}"), headers).await
            } else {
                http1(TokioIo::new(tls), FILE_HOST, path, headers).await
            }
        }
    }
}

fn request(
    uri: &str,
    host: Option<&str>,
    headers: &[(&'static str, String)],
) -> Result<hyper::Request<Empty<Bytes>>, SlackError> {
    let mut builder = hyper::Request::get(uri);
    if let Some(host) = host {
        builder = builder.header("Host", host);
    }
    for (name, value) in headers {
        builder = builder.header(*name, value);
    }
    builder.body(Empty::new()).map_err(|_| net("bad request"))
}

async fn http1<IO>(
    io: TokioIo<IO>,
    host: &str,
    path: &str,
    headers: &[(&'static str, String)],
) -> Result<Fetched, SlackError>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.map_err(|_| net("handshake failed"))?;
    let conn = AbortOnDrop(tokio::spawn(async move {
        let _ = conn.await;
    }));
    let answer = sender.send_request(request(path, Some(host), headers)?).await.map_err(|_| net("no answer"))?;
    Ok(fetched(answer, conn))
}

async fn http2<IO>(io: TokioIo<IO>, uri: &str, headers: &[(&'static str, String)]) -> Result<Fetched, SlackError>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, conn) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), io).await.map_err(|_| net("handshake failed"))?;
    let conn = AbortOnDrop(tokio::spawn(async move {
        let _ = conn.await;
    }));
    let answer = sender.send_request(request(uri, None, headers)?).await.map_err(|_| net("no answer"))?;
    Ok(fetched(answer, conn))
}

fn fetched(answer: hyper::Response<hyper::body::Incoming>, conn: AbortOnDrop<()>) -> Fetched {
    let header = |name: &str| answer.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let (location, content_type, retry_after) = (header("location"), header("content-type"), header("retry-after"));
    let length = header("content-length").and_then(|l| l.trim().parse().ok());
    let status = answer.status().as_u16();
    Fetched { status, location, content_type, length, retry_after, body: answer.into_body(), _conn: conn }
}
