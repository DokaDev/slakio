# ADR 0001: the HTTP stack of the Slack transport

Status: accepted (0.0.4).

## Context

`slakio-slack` must send requests that look like the official web client's: an exact header
set in an exact order, a form body in a fixed field order, nothing added (no `Accept-Encoding`,
no default `User-Agent`). It runs on the Tokio runtime the binary already has, and the binary
size is a CI budget.

## Decision

hyper 1 (`client` and `http1` features only), driven through its connection API
(`hyper::client::conn::http1`) with `hyper-util`'s Tokio adapter and `http-body-util`, one
connection per request. Decoding uses `serde` and `serde_json` (already in the workspace).

TLS joined with sign-in: rustls (ring provider) through `tokio-rustls`, with
`rustls-platform-verifier` (the OS's own certificate store and checks, no bundled root list),
offering HTTP/2 before HTTP/1.1 in ALPN as a browser does and speaking whichever the server
picks (`hyper::client::conn::http2` / `http1`). Only the workspace's own Slack host and the
edge cache's host are reached; a loopback address (the tests' fake Slack) is plain HTTP.

## Alternatives

- **reqwest**: a fine client, but it adds headers of its own, a connection pool, redirects and
  more dependencies; controlling the exact header order means working against it.
- **ureq**: small, but blocking (a thread per request beside the runtime) and with less control
  over headers.
- **A hand-written HTTP/1.1 client**: the fewest dependencies, but parsing HTTP correctly is
  exactly what a well-maintained crate is for. (The tests' fake server parses by hand, since it
  must record the raw header order and serves only the tests.)

## Consequences

- The transport decides every header and its order; hyper adds only `Host` (set by the
  transport) and the length. A test against the fake server checks the order on the wire.
- Dependencies are small and maintained by the Tokio project; `cargo deny` and `cargo audit`
  cover them in CI.
- TLS changed `http.rs` (and where requests go, `Endpoints`) only.
