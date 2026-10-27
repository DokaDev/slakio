//! Sending a [`Prepared`] request (hyper's connection API, one connection per request): the
//! headers go out exactly in the order given, nothing is added but the length (and, over
//! HTTP/1.1, the `Host`).
//!
//! Two servers can be reached, and nothing else: a workspace's Slack hosts — its own
//! (`<name>.slack.com`) and the edge cache's — over TLS (rustls, the OS's certificate verifier,
//! HTTP/2 when the server offers it as browsers use it, else HTTP/1.1); and, for the tests' fake
//! Slack, a loopback address over plain HTTP. The host of every request is checked again here,
//! where the socket opens.

use crate::error::SlackError;
use crate::request::{EDGE_HOST, Endpoints, Prepared, Server, workspace_host};
use crate::response::Raw;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
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

/// The host and the path (with the query) of an `https://` address.
fn split(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("https://")?;
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
