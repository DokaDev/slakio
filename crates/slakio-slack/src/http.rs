//! Sending a [`Prepared`] request (hyper's connection API, one connection per request): the
//! headers go out exactly in the order given, nothing is added but the length (and, over
//! HTTP/1.1, the `Host`).
//!
//! Two servers can be reached, and nothing else: a workspace's Slack hosts — its own
//! (`<name>.slack.com`) and the edge cache's — over TLS (rustls, the OS's certificate verifier,
//! HTTP/2 when the server offers it as browsers use it, else HTTP/1.1); and, for the tests' fake
//! Slack, a loopback address over plain HTTP. The host of every request is checked again here,
//! where the socket opens. The live updates' socket opens here too, to the realtime host only
//! ([`socket`]).

use crate::error::SlackError;
use crate::request::{EDGE_HOST, Endpoints, Opening, Prepared, SOCKET_HOST, Server, workspace_host};
use crate::response::Raw;
use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Full, Limited};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::net::TcpStream;

/// The longest a request may take, from connecting to the end of the answer.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// The largest answer read.
const BODY_MAX: usize = 32 * 1024 * 1024;

/// Send `request` to the server of `endpoints`. Crate-private: only [`crate::Client`] sends,
/// after the allowlist and the governor.
pub(crate) async fn send(endpoints: &Endpoints, request: &Prepared) -> Result<Raw, SlackError> {
    match tokio::time::timeout(TIMEOUT, exchange(endpoints, request)).await {
        Ok(answer) => answer,
        Err(_) => Err(net("timed out")),
    }
}

fn net(detail: &str) -> SlackError {
    SlackError::Network { detail: detail.to_string() }
}

const ONLY_LOCAL: SlackError = SlackError::Offline { why: "only a server on this machine can be reached" };
const ONLY_SLACK: SlackError = SlackError::Offline { why: "only the workspace's Slack hosts can be reached" };

async fn exchange(endpoints: &Endpoints, request: &Prepared) -> Result<Raw, SlackError> {
    match &endpoints.server {
        Server::Local(addr) => {
            if !addr.ip().is_loopback() {
                return Err(ONLY_LOCAL);
            }
            let Some(path) = request.url.strip_prefix(&format!("http://{addr}")) else { return Err(ONLY_LOCAL) };
            let stream = TcpStream::connect(addr).await.map_err(|_| net("could not connect"))?;
            http1(TokioIo::new(stream), &addr.to_string(), path, request).await
        }
        Server::Slack { api_host } => {
            let (host, path) = split(&request.url).ok_or(ONLY_SLACK)?;
            if !(host == api_host && workspace_host(host) || host == EDGE_HOST) {
                return Err(ONLY_SLACK);
            }
            let tcp = TcpStream::connect((host, 443)).await.map_err(|_| net("could not connect"))?;
            over_tls(tcp, host, config()?, path, request).await
        }
    }
}

/// An open socket of the live updates.
pub(crate) type Upgraded = TokioIo<hyper::upgrade::Upgraded>;

/// Open the live updates' socket with `opening`: to the realtime host over TLS (HTTP/1.1, the
/// only way a socket opens), or to the tests' fake on a loopback address. The server must agree
/// to the socket (`101`, with the key's digest); anything else fails. Crate-private: only
/// [`crate::Client`] opens one, after the governor.
pub(crate) async fn socket(endpoints: &Endpoints, opening: &Opening) -> Result<Upgraded, SlackError> {
    match tokio::time::timeout(TIMEOUT, open(endpoints, opening)).await {
        Ok(socket) => socket,
        Err(_) => Err(net("timed out")),
    }
}

async fn open(endpoints: &Endpoints, opening: &Opening) -> Result<Upgraded, SlackError> {
    match &endpoints.server {
        Server::Local(addr) => {
            if !addr.ip().is_loopback() {
                return Err(ONLY_LOCAL);
            }
            let Some(path) = opening.url.strip_prefix(&format!("ws://{addr}")) else { return Err(ONLY_LOCAL) };
            let stream = TcpStream::connect(addr).await.map_err(|_| net("could not connect"))?;
            upgrade(TokioIo::new(stream), &addr.to_string(), path, opening).await
        }
        Server::Slack { .. } => {
            let (host, path) = split_as("wss://", &opening.url).ok_or(ONLY_SLACK)?;
            if host != SOCKET_HOST {
                return Err(ONLY_SLACK);
            }
            let tcp = TcpStream::connect((host, 443)).await.map_err(|_| net("could not connect"))?;
            let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|_| ONLY_SLACK)?;
            let tls = tokio_rustls::TlsConnector::from(socket_config()?)
                .connect(name, tcp)
                .await
                .map_err(|_| net("the secure connection failed"))?;
            upgrade(TokioIo::new(tls), host, path, opening).await
        }
    }
}

/// The opening request on `io` to `host`, and the socket once the server agreed.
async fn upgrade<IO>(io: TokioIo<IO>, host: &str, path: &str, opening: &Opening) -> Result<Upgraded, SlackError>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.map_err(|_| net("handshake failed"))?;
    // Driven until the socket is handed over (or this ends first).
    let _conn = AbortOnDrop(tokio::spawn(conn.with_upgrades()));
    let mut builder = hyper::Request::get(path).header("Host", host);
    for (name, value) in &opening.headers {
        builder = builder.header(*name, value);
    }
    let request = builder.body(Empty::<Bytes>::new()).map_err(|_| net("bad request"))?;
    let answer = sender.send_request(request).await.map_err(|_| net("no answer"))?;
    let agreed = answer.status() == hyper::StatusCode::SWITCHING_PROTOCOLS
        && header_is(&answer, "upgrade", "websocket")
        && header_is(&answer, "connection", "upgrade")
        && answer.headers().get("sec-websocket-accept").and_then(|v| v.to_str().ok())
            == Some(crate::live::socket::accept(&opening.key).as_str())
        // No extension or subprotocol was asked for: one the server picks anyway cannot be read.
        && answer.headers().get("sec-websocket-extensions").is_none()
        && answer.headers().get("sec-websocket-protocol").is_none();
    if answer.status() == hyper::StatusCode::TOO_MANY_REQUESTS {
        let after = answer.headers().get("retry-after").and_then(|v| v.to_str().ok());
        return Err(SlackError::RateLimited { retry_after: crate::response::retry_after(after) });
    }
    // Slack no longer takes the sign-in the socket was asked with.
    if answer.status() == hyper::StatusCode::UNAUTHORIZED {
        return Err(SlackError::Auth { code: "the socket was refused (401)".to_string() });
    }
    if !agreed {
        return Err(SlackError::Network { detail: format!("the socket was refused ({})", answer.status().as_u16()) });
    }
    let upgraded = hyper::upgrade::on(answer).await.map_err(|_| net("the socket did not open"))?;
    Ok(TokioIo::new(upgraded))
}

/// Header `name` of `answer` holds the word `word` (in any case, among comma-separated words).
fn header_is(answer: &hyper::Response<hyper::body::Incoming>, name: &str, word: &str) -> bool {
    answer
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|w| w.trim().eq_ignore_ascii_case(word)))
}

/// The TLS settings of the socket: as [`config`]'s, but HTTP/1.1 only (a socket opens over it).
fn socket_config() -> Result<Arc<rustls::ClientConfig>, SlackError> {
    static CONFIG: OnceLock<Option<Arc<rustls::ClientConfig>>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut config = (*config().ok()?).clone();
            config.alpn_protocols = vec![b"http/1.1".to_vec()];
            Some(Arc::new(config))
        })
        .clone()
        .ok_or_else(|| net("TLS is not available on this system"))
}

/// The host and the path (with the query) of an `https://` address.
fn split(url: &str) -> Option<(&str, &str)> {
    split_as("https://", url)
}

/// The host and the path (with the query) of an address of `scheme`.
fn split_as<'a>(scheme: &str, url: &'a str) -> Option<(&'a str, &'a str)> {
    let rest = url.strip_prefix(scheme)?;
    let slash = rest.find('/')?;
    Some((&rest[..slash], &rest[slash..]))
}

/// The client's TLS settings, made once: the ring provider, the OS's verifier, HTTP/2 offered
/// before HTTP/1.1 (as a browser does).
fn config() -> Result<Arc<rustls::ClientConfig>, SlackError> {
    use rustls_platform_verifier::BuilderVerifierExt;
    static CONFIG: OnceLock<Option<Arc<rustls::ClientConfig>>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let mut config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .ok()?
                .with_platform_verifier()
                .ok()?
                .with_no_client_auth();
            config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
            Some(Arc::new(config))
        })
        .clone()
        .ok_or_else(|| net("TLS is not available on this system"))
}

/// `request` over TLS on `tcp` to `host`, with `config`.
pub(crate) async fn over_tls(
    tcp: TcpStream,
    host: &str,
    config: Arc<rustls::ClientConfig>,
    path: &str,
    request: &Prepared,
) -> Result<Raw, SlackError> {
    let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|_| ONLY_SLACK)?;
    let tls = tokio_rustls::TlsConnector::from(config)
        .connect(name, tcp)
        .await
        .map_err(|_| net("the secure connection failed"))?;
    if tls.get_ref().1.alpn_protocol() == Some(b"h2") {
        http2(TokioIo::new(tls), &format!("https://{host}{path}"), request).await
    } else {
        http1(TokioIo::new(tls), host, path, request).await
    }
}

/// The request's headers in order, with the length after the content type.
fn build(uri: &str, host: Option<&str>, request: &Prepared) -> Result<hyper::Request<Full<Bytes>>, SlackError> {
    let mut builder = hyper::Request::post(uri);
    if let Some(host) = host {
        builder = builder.header("Host", host);
    }
    for (name, value) in &request.headers {
        builder = builder.header(*name, value);
        if *name == "Content-Type" {
            builder = builder.header("Content-Length", request.body.len());
        }
    }
    builder.body(Full::new(Bytes::from(request.body.clone()))).map_err(|_| net("bad request"))
}

async fn http1<IO>(io: TokioIo<IO>, host: &str, path: &str, request: &Prepared) -> Result<Raw, SlackError>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.map_err(|_| net("handshake failed"))?;
    // The connection's task ends with this function, however it ends: an error, the timeout,
    // or the kill switch dropping the call.
    let _conn = AbortOnDrop(tokio::spawn(conn));
    let answer = sender.send_request(build(path, Some(host), request)?).await.map_err(|_| net("no answer"))?;
    read(answer).await
}

async fn http2<IO>(io: TokioIo<IO>, uri: &str, request: &Prepared) -> Result<Raw, SlackError>
where
    IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, conn) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), io).await.map_err(|_| net("handshake failed"))?;
    let _conn = AbortOnDrop(tokio::spawn(conn));
    // HTTP/2 names the host in the address (`:authority`), not in a header.
    let answer = sender.send_request(build(uri, None, request)?).await.map_err(|_| net("no answer"))?;
    read(answer).await
}

async fn read(answer: hyper::Response<hyper::body::Incoming>) -> Result<Raw, SlackError> {
    let status = answer.status().as_u16();
    let retry_after = answer.headers().get("retry-after").and_then(|v| v.to_str().ok()).map(str::to_string);
    let body = Limited::new(answer.into_body(), BODY_MAX).collect().await.map_err(|_| net("answer cut off"))?;
    Ok(Raw { status, retry_after, body: body.to_bytes().to_vec() })
}

/// A task aborted when this is dropped.
struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests;
