//! UI-free and network-free core of slakio: what the terminal UI and the adapters (Slack
//! client, authentication, local store) share.
//!
//! * [`model`] — the domain model: workspaces, people, sections, conversations, messages.
//! * [`backend`] — the command/event protocol between the UI and a backend.
//! * [`paths`] — the config, data, state and cache directories; [`fsutil`] — atomic file writes.
//! * [`config`] — the config file.
//! * [`secret`] — the secret store interface, its in-memory implementation and the
//!   `SLAKIO_SECRET_STORE` switch.
//! * [`sanitize`] — the terminal-escape sanitiser and the [`sanitize::Remote`] text type that
//!   makes it mandatory for anything remote.
//! * [`fault`] — failures as data (a kind the UI words, a detail for the error log).
//! * [`i18n`] — the embedded en/ko message catalogs.
//!
//! This crate must never depend on a UI crate (ratatui/crossterm), a network or database crate,
//! or another slakio crate.

pub mod backend;
pub mod config;
pub mod fault;
pub mod fsutil;
pub mod i18n;
pub mod model;
pub mod paths;
pub mod sanitize;
pub mod secret;
