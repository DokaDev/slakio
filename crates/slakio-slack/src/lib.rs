//! slakio's Slack transport, read-only by construction:
//!
//! * [`method`] — the allowlist: the read-only methods that may be called, and nothing else;
//! * [`request`] — requests in the official web client's format (headers, token in the body,
//!   the `_x_*` parameters in order);
//! * [`response`] and [`wire`] — Slack's answers, decoded tolerantly, and its errors as data
//!   ([`error::SlackError`], worded by the core's `FailKind`);
//! * [`governor`] — the process-wide rails: the kill switch, the request cap, at most eight
//!   requests in flight;
//! * `http` (crate-private) — the wire (this version reaches a server on this machine only:
//!   no TLS, so no Slack);
//! * [`client`] — one workspace's calls through all of the above; [`adapter`] — the backend
//!   behind the UI's handle.
//!
//! **The only way out is [`Client`]** (and [`serve`], which uses it): a request is built from
//! an allowed [`Method`] and sent only after the governor lets it through. Building a request
//! by hand and sending it is not possible from outside the crate:
//!
//! ```compile_fail
//! // The wire is crate-private.
//! use slakio_slack::http::send;
//! ```
//!
//! ```compile_fail
//! // So is a prepared request and its builder.
//! use slakio_slack::request::{Prepared, prepare};
//! ```
//!
//! The crate depends on `slakio-core` only. The terminal UI never names it but in the binary's
//! wiring; the authentication crate never depends on it (the binary hands it the credentials).

pub mod adapter;
pub mod client;
pub mod error;
pub mod governor;
pub(crate) mod http;
pub mod method;
pub mod request;
pub mod response;
pub mod wire;

pub use adapter::serve;
pub use client::Client;
pub use error::SlackError;
pub use governor::{Governor, Limits, Priority};
pub use method::Method;
