//! Sending a [`Prepared`] request over HTTP/1.1 (hyper's connection API, one connection per
//! request): the headers go out exactly in the order given, nothing is added but the
//! `Host` and the length. This version speaks plain HTTP to the loopback address only (the
//! tests' fake Slack); it holds no TLS, so it cannot reach Slack.

use crate::error::SlackError;
use crate::request::{Endpoints, Prepared};
use crate::response::Raw;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper_util::rt::TokioIo;
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

async fn exchange(endpoints: &Endpoints, request: &Prepared) -> Result<Raw, SlackError> {
    // The address is checked again where the socket opens: nothing but loopback, ever.
    if !endpoints.addr.ip().is_loopback() {
        return Err(SlackError::Offline { why: "only a server on this machine can be reached" });
    }
    let origin = format!("http://{}", endpoints.addr);
    let Some(path) = request.url.strip_prefix(&origin) else {
        return Err(SlackError::Offline { why: "only a server on this machine can be reached" });
    };
    let stream = TcpStream::connect(endpoints.addr).await.map_err(|_| net("could not connect"))?;
    let (mut sender, conn) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await.map_err(|_| net("handshake failed"))?;
    // The connection's task ends with this function, however it ends: an error, the timeout,
    // or the kill switch dropping the call.
    let _conn = AbortOnDrop(tokio::spawn(conn));

    let mut builder = hyper::Request::post(path).header("Host", endpoints.addr.to_string());
    for (name, value) in &request.headers {
        builder = builder.header(*name, value);
        if *name == "Content-Type" {
            builder = builder.header("Content-Length", request.body.len());
        }
    }
    let req = builder.body(Full::new(Bytes::from(request.body.clone()))).map_err(|_| net("bad request"))?;
    let answer = sender.send_request(req).await.map_err(|_| net("no answer"))?;
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
